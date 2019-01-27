use std::collections::vec_deque::VecDeque;
use std::slice::Iter;
use std::vec::Vec;

use analysis::function_parameters::{async_param_to_remove, CParameter};
use analysis::functions::{find_function, find_index_to_ignore, finish_function_name};
use analysis::imports::Imports;
use analysis::out_parameters::use_function_return_for_result;
use analysis::rust_type::{bounds_rust_type, rust_type, rust_type_with_scope};
use consts::TYPE_PARAMETERS_START;
use env::Env;
use library::{Class, Concurrency, Function, Fundamental, Nullable, ParameterDirection, Type, TypeId};
use traits::IntoString;

#[derive(Clone, Eq, Debug, PartialEq)]
pub enum BoundType {
    NoWrapper,
    // lifetime
    IsA(Option<char>),
    // lifetime <- shouldn't be used but just in case...
    AsRef(Option<char>),
    // lifetime (if none, not a reference, like for primitive types), wrapper type
    Into(Option<char>, Option<Box<BoundType>>),
}

impl BoundType {
    pub fn is_into(&self) -> bool {
        match *self {
            BoundType::Into(_, _) => true,
            _ => false,
        }
    }

    pub fn need_isa(&self) -> bool {
        match *self {
            BoundType::IsA(_) => true,
            BoundType::Into(_, Some(ref bound_type)) if bound_type.need_isa() => true,
            _ => false,
        }
    }

    fn with_lifetime(ty_: BoundType, lifetime: char) -> BoundType {
        match ty_ {
            BoundType::NoWrapper => unreachable!(),
            BoundType::IsA(_) => BoundType::IsA(Some(lifetime)),
            BoundType::AsRef(_) => BoundType::AsRef(Some(lifetime)),
            BoundType::Into(_, x) => BoundType::Into(Some(lifetime), x),
        }
    }
}

#[derive(Clone, Eq, Debug, PartialEq)]
pub struct Bound {
    pub bound_type: BoundType,
    pub parameter_name: String,
    pub alias: char,
    pub type_str: String,
    pub info_for_next_type: bool,
    pub callback_modified: bool,
}

#[derive(Clone, Debug)]
pub struct Bounds {
    unused: VecDeque<char>,
    used: Vec<Bound>,
    unused_lifetimes: VecDeque<char>,
    lifetimes: Vec<char>,
}

impl Default for Bounds {
    fn default() -> Bounds {
        Bounds {
            unused: (TYPE_PARAMETERS_START as u8..)
                .take_while(|x| *x <= b'Z')
                .map(|x| x as char)
                .collect(),
            used: Vec::new(),
            unused_lifetimes: "abcdefg".chars().collect(),
            lifetimes: Vec::new(),
        }
    }
}

impl Bound {
    pub fn get_for_property_setter(
        env: &Env,
        var_name: &str,
        type_id: TypeId,
        nullable: Nullable,
    ) -> Option<Bound> {
        if let Some(bound_type) = Bounds::type_for(env, type_id, nullable) {
            if bound_type.is_into() {
                //TODO: match boxed_bound to BoundType::IsA(l)
                let type_str = bounds_rust_type(env, type_id);
                return Some(Bound {
                    bound_type,
                    parameter_name: var_name.to_owned(),
                    alias: TYPE_PARAMETERS_START,
                    type_str: type_str.into_string(),
                    info_for_next_type: false,
                    callback_modified: false,
                })
            }
        }
        None
    }
}

#[derive(Debug, Clone)]
pub struct CallbackInfo {
    pub callback_type: String,
    pub success_parameters: String,
    pub error_parameters: String,
    pub bound_name: char,
}

impl Bounds {
    pub fn add_for_parameter(
        &mut self,
        env: &Env,
        func: &Function,
        par: &CParameter,
        async: bool,
        concurrency: Concurrency,
    ) -> (Option<String>, Option<CallbackInfo>) {
        let type_name = bounds_rust_type(env, par.typ);
        let mut type_string = if async && async_param_to_remove(&par.name) {
            return (None, None);
        } else if type_name.is_err() {
            return (None, None)
        } else {
            type_name.into_string()
        };
        let mut callback_info = None;
        let mut ret = None;
        let mut need_is_into_check = false;
        if !par.instance_parameter && par.direction != ParameterDirection::Out {
            if let Some(bound_type) = Bounds::type_for(env, par.typ, par.nullable) {
                ret = Some(Bounds::get_to_glib_extra(&bound_type));
                if async && par.name == "callback" {
                    let func_name = func.c_identifier.as_ref().unwrap();
                    let finish_func_name = finish_function_name(func_name);
                    if let Some(function) = find_function(env, &finish_func_name) {
                        let mut out_parameters = find_out_parameters(env, function);
                        if use_function_return_for_result(env, function.ret.typ) {
                            out_parameters
                                .insert(0, rust_type(env, function.ret.typ).into_string());
                        }
                        let parameters = format_out_parameters(&out_parameters);
                        let error_type = find_error_type(env, function);
                        type_string = format!(
                            "FnOnce(Result<{}, {}>) + Send + 'static",
                            parameters, error_type
                        );
                        let bound_name = *self.unused.front().unwrap();
                        callback_info = Some(CallbackInfo {
                            callback_type: type_string.clone(),
                            success_parameters: parameters,
                            error_parameters: error_type,
                            bound_name,
                        });
                    }
                } else if par.c_type == "GDestroyNotify" ||
                          env.library.type_(par.typ).is_function() {
                    need_is_into_check = par.c_type != "GDestroyNotify";

                    let mut extra_bounds = Vec::new();
                    if par.c_type != "GDestroyNotify" {
                        if let Type::Function(ref f) = env.library.type_(par.typ) {
                            for (pos, param) in f.parameters.iter().enumerate() {
                                let type_str = rust_type(env, param.typ).into_string();
                                // If there is already a IsA for this type, no need to add
                                // another one!
                                if let Some(sub_pos) = self.used.iter()
                                                                .enumerate()
                                                                .find_map(|(p, c)| {
                                                                    if par.c_type == "GtkTreeCellDataFunc" {
                                                                        println!("{} == {} => {}", c.type_str, type_str, c.type_str == type_str);
                                                                    }
                                                                    if c.type_str == type_str {
                                                                        Some(p)
                                                                    } else {
                                                                        None
                                                                    }
                                                                }) {
                                    if par.c_type == "GtkTreeCellDataFunc" {
                                        println!("result: {}", pos);
                                    }
                                    extra_bounds.push(sub_pos);
                                    continue;
                                }
                                if let Some(BoundType::IsA(None)) = Self::type_for(&env, param.typ, param.nullable) {
                                    let info = Bound {
                                        bound_type: BoundType::IsA(None),
                                        parameter_name: param.name.clone(),
                                        alias: self.unused.pop_front().unwrap(),
                                        type_str,
                                        info_for_next_type: false,
                                        callback_modified: false,
                                    };
                                    extra_bounds.push(self.used.len());
                                    self.used.push(info);
                                }
                            }
                        }
                    }
                    if par.c_type == "GtkTreeCellDataFunc" {
                        println!("1. {:?}", extra_bounds.iter().map(|p| &self.used[*p]).collect::<Vec<_>>().as_slice());
                        println!("2. {:?}", extra_bounds);
                    }
                    type_string = rust_type_with_scope(
                        env, par.typ, par.scope, concurrency,
                        extra_bounds.iter().map(|p| &self.used[*p]).collect::<Vec<_>>().as_slice())
                    .into_string();
                    let bound_name = *self.unused.front().unwrap();
                    callback_info = Some(CallbackInfo {
                        callback_type: type_string.clone(),
                        success_parameters: String::new(),
                        error_parameters: String::new(),
                        bound_name,
                    });
                }
                if par.c_type != "GDestroyNotify" &&
                   !self.add_parameter(&par.name, &type_string, bound_type, async) {
                    panic!(
                        "Too many type constraints for {}",
                        func.c_identifier.as_ref().unwrap()
                    )
                }
                if need_is_into_check {
                    if let Some(x) = if let Some(ref mut last) = self.used.last_mut() {
                        if last.bound_type.is_into() {
                            let mut new_one = (*last).clone();
                            new_one.alias = self.unused.pop_front().expect("no available bound");
                            new_one.type_str = last.alias.to_string();
                            new_one.parameter_name = last.parameter_name.clone();
                            // When we create a new bound for a callback which can be NULL,
                            // we need to generate two new bounds instead of just one. This flag
                            // allows us to know it so we can prevent its "generation" in the
                            // codegen part (we don't need the `Into<>` part in a few parts of the
                            // code).
                            new_one.callback_modified = true;

                            last.bound_type = BoundType::NoWrapper;
                            last.parameter_name = String::new();

                            Some(new_one)
                        } else {
                            None
                        }
                    } else {
                        None
                    } {
                        self.used.push(x);
                    }
                }
            }
        } else if par.instance_parameter {
            if let Some(bound_type) = Bounds::type_for(env, par.typ, par.nullable) {
                ret = Some(Bounds::get_to_glib_extra(&bound_type));
            }
        }

        (ret, callback_info)
    }

    pub fn type_for(env: &Env, type_id: TypeId, nullable: Nullable) -> Option<BoundType> {
        use self::BoundType::*;
        match *env.library.type_(type_id) {
            Type::Fundamental(Fundamental::Filename) => Some(AsRef(None)),
            Type::Fundamental(Fundamental::OsString)=> Some(AsRef(None)),
            Type::Fundamental(Fundamental::Utf8) if *nullable => Some(Into(Some('_'), None)),
            Type::Class(Class { final_type, ..}) if !*nullable => {
                if final_type {
                    None
                } else {
                    Some(IsA(None))
                }
            },
            Type::Class(Class { final_type, ..}) => {
                if final_type {
                    Some(Into(Some('_'), None))
                } else {
                    Some(Into(Some('_'), Some(Box::new(IsA(None)))))
                }
            },
            Type::Interface(..) if !*nullable => Some(IsA(None)),
            Type::Interface(..) => Some(Into(Some('_'), Some(Box::new(IsA(None))))),
            Type::List(_) | Type::SList(_) | Type::CArray(_) => None,
            Type::Fundamental(_) if *nullable => Some(Into(None, None)),
            Type::Function(_) if *nullable => Some(Into(None, None)),
            Type::Function(_) if !*nullable => Some(NoWrapper),
            _ if !*nullable => None,
            _ => Some(Into(Some('_'), None)),
        }
    }
    fn get_to_glib_extra(bound_type: &BoundType) -> String {
        use self::BoundType::*;
        match *bound_type {
            AsRef(_) => ".as_ref()".to_owned(),
            IsA(_) => ".as_ref()".to_owned(),
            Into(_, Some(ref x)) => Bounds::get_to_glib_extra(x),
            _ => String::new(),
        }
    }
    pub fn add_parameter(&mut self, name: &str, type_str: &str, mut bound_type: BoundType, async: bool) -> bool {
        if async && name == "callback" {
            if let Some(alias) = self.unused.pop_front() {
                self.used.push(Bound {
                    bound_type: BoundType::NoWrapper,
                    parameter_name: name.to_owned(),
                    alias,
                    type_str: type_str.to_string(),
                    info_for_next_type: false,
                    callback_modified: false,
                });
                return true;
            }
            return false;
        }
        if self.used.iter().any(|n| n.parameter_name == name) {
            return false;
        }
        let sub = if let BoundType::Into(Some(_), x) = bound_type {
            if let Some(lifetime) = self.unused_lifetimes.pop_front() {
                self.lifetimes.push(lifetime);
                bound_type = BoundType::Into(Some(lifetime), x.clone());
                Some((x, lifetime))
            } else {
                return false;
            }
        } else {
            None
        };
        let type_str = if let Some((Some(sub), lifetime)) = sub {
            if let Some(alias) = self.unused.pop_front() {
                self.used.push(Bound {
                    bound_type: BoundType::with_lifetime(*sub, lifetime),
                    parameter_name: name.to_owned(),
                    alias,
                    type_str: type_str.to_owned(),
                    info_for_next_type: true,
                    callback_modified: false,
                });
                alias.to_string()
            } else {
                return false;
            }
        } else {
            type_str.to_owned()
        };
        if let Some(alias) = self.unused.pop_front() {
            self.used.push(Bound {
                bound_type,
                parameter_name: name.to_owned(),
                alias,
                type_str: type_str.to_owned(),
                info_for_next_type: false,
                callback_modified: false,
            });
            true
        } else {
            false
        }
    }
    pub fn get_parameter_alias_info(&self, name: &str) -> Option<(char, BoundType)> {
        self.used
            .iter()
            .find(move |n| {
                if n.parameter_name == name {
                    !n.info_for_next_type
                } else {
                    false
                }})
            .map(|t| (t.alias, t.bound_type.clone()))
    }
    pub fn get_base_alias(&self, alias: char) -> Option<char> {
        if alias == TYPE_PARAMETERS_START {
            return None;
        }
        let prev_alias = ((alias as u8) - 1) as char;
        self.used
            .iter()
            .find(move |n| n.alias == prev_alias)
            .and_then(|b| if b.info_for_next_type {
                Some(b.alias)
            } else {
                None
            })
    }
    pub fn update_imports(&self, imports: &mut Imports) {
        //TODO: import with versions
        use self::BoundType::*;
        for used in &self.used {
            match used.bound_type {
                NoWrapper => (),
                IsA(_) => imports.add("glib::object::IsA", None),
                AsRef(_) => imports.add_used_type(&used.type_str, None),
                Into(_, Some(ref x)) => {
                    match **x {
                        IsA(_) => imports.add("glib::object::IsA", None),
                        // This case shouldn't be possible normally.
                        AsRef(_) => imports.add_used_type(&used.type_str, None),
                        _ => {}
                    }
                }
                Into(_, None) => {}
            }
        }
    }
    pub fn is_empty(&self) -> bool {
        self.used.is_empty()
    }
    pub fn iter(&self) -> Iter<Bound> {
        self.used.iter()
    }
    pub fn iter_lifetimes(&self) -> Iter<char> {
        self.lifetimes.iter()
    }
}

fn find_out_parameters(env: &Env, function: &Function) -> Vec<String> {
    let index_to_ignore = find_index_to_ignore(&function.parameters);
    function.parameters.iter().enumerate()
        .filter(|&(index, param)|
                Some(index) != index_to_ignore &&
                param.direction == ParameterDirection::Out &&
                param.name != "error")
        .map(|(_, param)| rust_type(env, param.typ).expect("get rust type from param"))
        .collect()
}

fn format_out_parameters(parameters: &[String]) -> String {
    if parameters.len() == 1 {
        parameters[0].to_string()
    } else {
        format!("({})", parameters.join(", "))
    }
}

fn find_error_type(env: &Env, function: &Function) -> String {
    let error_param = function.parameters.iter()
        .find(|param| param.direction == ParameterDirection::Out
                && param.name == "error")
        .expect("error type");
    if let Type::Record(ref record) = *env.type_(error_param.typ) {
        return record.name.clone();
    }
    panic!("cannot find error type")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_new_all() {
        let mut bounds: Bounds = Default::default();
        let typ = BoundType::IsA(None);
        assert_eq!(bounds.add_parameter("a", "", typ.clone(), false), true);
        // Don't add second time
        assert_eq!(bounds.add_parameter("a", "", typ.clone(), false), false);
        assert_eq!(bounds.add_parameter("b", "", typ.clone(), false), true);
        assert_eq!(bounds.add_parameter("c", "", typ.clone(), false), true);
        assert_eq!(bounds.add_parameter("d", "", typ.clone(), false), true);
        assert_eq!(bounds.add_parameter("e", "", typ.clone(), false), true);
        assert_eq!(bounds.add_parameter("f", "", typ.clone(), false), true);
        assert_eq!(bounds.add_parameter("g", "", typ.clone(), false), true);
        assert_eq!(bounds.add_parameter("h", "", typ.clone(), false), true);
        assert_eq!(bounds.add_parameter("h", "", typ.clone(), false), false);
        assert_eq!(bounds.add_parameter("i", "", typ.clone(), false), true);
        assert_eq!(bounds.add_parameter("j", "", typ.clone(), false), true);
        assert_eq!(bounds.add_parameter("k", "", typ.clone(), false), true);
        assert_eq!(bounds.add_parameter("l", "", typ, false), false);
    }

    #[test]
    fn get_parameter_alias_info() {
        let mut bounds: Bounds = Default::default();
        let typ = BoundType::IsA(None);
        bounds.add_parameter("a", "", typ.clone(), false);
        bounds.add_parameter("b", "", typ.clone(), false);
        assert_eq!(
            bounds.get_parameter_alias_info("a"),
            Some(('P', typ.clone()))
        );
        assert_eq!(bounds.get_parameter_alias_info("b"), Some(('Q', typ)));
        assert_eq!(bounds.get_parameter_alias_info("c"), None);
    }
}
