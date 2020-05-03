use super::{
    general::{cfg_deprecated, doc_hidden, version_condition},
    trampoline::{self, func_string},
};
use crate::{
    analysis,
    chunk::Chunk,
    consts::TYPE_PARAMETERS_START,
    env::Env,
    library,
    writer::{primitives::tabs, ToCode},
};
use std::io::{Result, Write};

pub fn generate(
    w: &mut dyn Write,
    env: &Env,
    analysis: &analysis::signals::Info,
    trait_name: Option<&str>,
    only_declaration: bool,
    indent: usize,
) -> Result<()> {
    let commented = analysis.trampoline.is_err();
    let comment_prefix = if commented { "//" } else { "" };
    let pub_prefix = if trait_name.is_some() { "" } else { "pub " };

    let function_type = function_type_string(env, analysis, true);
    let declaration = declaration(analysis, &function_type);
    let suffix = if only_declaration { ";" } else { " {" };

    writeln!(w)?;
    if trait_name.is_none() || only_declaration {
        cfg_deprecated(w, env, analysis.deprecated_version, commented, indent)?;
    }
    version_condition(w, env, analysis.version, commented, indent)?;
    doc_hidden(w, analysis.doc_hidden, comment_prefix, indent)?;
    writeln!(
        w,
        "{}{}{}{}{}",
        tabs(indent),
        comment_prefix,
        pub_prefix,
        declaration,
        suffix
    )?;

    if !only_declaration {
        if !commented {
            if let Ok(ref trampoline) = analysis.trampoline {
                trampoline::generate(w, env, trampoline, trait_name.is_some(), 2)?;
            }
        }
        match function_type {
            Some(_) => {
                let body = body(analysis, trait_name, &env).to_code(env);
                for s in body {
                    for line in s.lines() {
                        // println!("=> {:?}", line);
                        writeln!(w, "{}{}", tabs(indent), line)?;
                    }
                }
                writeln!(w, "{}}}", tabs(indent))?;
            }
            _ => {
                if let Err(ref errors) = analysis.trampoline {
                    for error in errors {
                        writeln!(w, "{}{}\t{}", tabs(indent), comment_prefix, error)?;
                    }
                    writeln!(w, "{}{}}}", tabs(indent), comment_prefix)?;
                } else {
                    writeln!(
                        w,
                        "{}{}\tTODO: connect to trampoline\n{0}{1}}}",
                        tabs(indent),
                        comment_prefix
                    )?;
                }
            }
        }
    }

    if function_type.is_none() {
        // Signal incomplete, can't generate emit
        return Ok(());
    }

    if let Some(ref emit_name) = analysis.action_emit_name {
        writeln!(w)?;
        if trait_name.is_none() || only_declaration {
            cfg_deprecated(w, env, analysis.deprecated_version, commented, indent)?;
        }
        version_condition(w, env, analysis.version, commented, indent)?;

        let function_type = function_type_string(env, analysis, false);

        writeln!(
            w,
            "{}{}{}fn {}{}{}",
            tabs(indent),
            comment_prefix,
            pub_prefix,
            emit_name,
            function_type.unwrap(),
            suffix
        )?;

        if !only_declaration {
            let trampoline = analysis.trampoline.as_ref().unwrap_or_else(|_| {
                panic!(
                    "Internal error: can't find trampoline for signal '{}'",
                    analysis.signal_name,
                )
            });
            let mut args = String::with_capacity(100);

            for (pos, par) in trampoline.parameters.rust_parameters.iter().enumerate() {
                // Skip the self parameter
                if pos == 0 {
                    continue;
                }

                if pos > 1 {
                    args.push_str(", ");
                }
                args.push('&');
                args.push_str(&par.name);
            }

            writeln!(
                w,
                "{}let {} = unsafe {{ glib::Object::from_glib_borrow(self.to_glib_none().0 as *mut gobject_sys::GObject).emit(\"{}\", &[{}]).unwrap() }};",
                tabs(indent + 1),
                if trampoline.ret.typ != Default::default() {
                    "res"
                } else {
                    "_"
                },
                analysis.signal_name,
                args,
            )?;

            if trampoline.ret.typ != Default::default() {
                let unwrap = if trampoline.ret.nullable == library::Nullable(true) {
                    ""
                } else {
                    ".unwrap()"
                };

                writeln!(
                    w,
                    "{}res.unwrap().get().expect(\"Return Value for `{}`\"){}",
                    tabs(indent + 1),
                    emit_name,
                    unwrap,
                )?;
            }
            writeln!(w, "{}}}", tabs(indent))?;
        }
    }

    Ok(())
}

fn function_type_string(
    env: &Env,
    analysis: &analysis::signals::Info,
    closure: bool,
) -> Option<String> {
    if analysis.trampoline.is_err() {
        return None;
    }

    let trampoline = analysis.trampoline.as_ref().unwrap_or_else(|_| {
        panic!(
            "Internal error: can't find trampoline for signal '{}'",
            analysis.signal_name
        )
    });

    let type_ = func_string(
        env,
        trampoline,
        if closure {
            Some((TYPE_PARAMETERS_START, "Self"))
        } else {
            Some((TYPE_PARAMETERS_START, "self"))
        },
        closure,
    );
    Some(type_)
}

fn declaration(analysis: &analysis::signals::Info, function_type: &Option<String>) -> String {
    let bounds = bounds(function_type);
    let param_str = "&self, f: F";
    let return_str = " -> SignalHandlerId";
    format!(
        "fn {}<{}>({}){}",
        analysis.connect_name, bounds, param_str, return_str
    )
}

fn bounds(function_type: &Option<String>) -> String {
    match *function_type {
        Some(ref type_) => format!("F: {}", type_),
        _ => "Unsupported or ignored types".to_owned(),
    }
}

fn body(analysis: &analysis::signals::Info, trait_name: Option<&str>, env: &Env) -> Chunk {
    if let Ok(ref trampoline) = analysis.trampoline {
        // if trampoline.is_notify {
        //     return format!(
        //         "{}, _param_spec: glib_sys::gpointer",
        //         trampoline_parameter(env, &analysis.parameters.c_parameters[0])
        //     );
        // }

        let mut parameter_strs: Vec<String> = Vec::new();

        let mut s = format!(r#"    self.connect_local("{0}", false, move |values| {{"#, analysis.signal_name);
        // format!(r#"    self.connect_local("{0}", false, move |values| {{
        // let obj_instance: Self = values[0]
        //     .get::<glib::Object>()
        //     .expect("Wrong argument type for first closure argument")
        //     .downcast::<Self>()
        //     .expect("Failed to downcast to {1}");"#, analysis.signal_name, analysis.obj_name);

        for (pos, par) in trampoline.parameters.rust_parameters.iter().enumerate() {
            if pos == 0 && par.name == "this" {
                s.push_str(&format!(r#"
        let {0} = values[{1}]
            .get::<glib::Object>()
            .expect("Wrong argument for closure argument number {1}")
            .expect("Still wroooong")
            .downcast::<Self>()
            .expect("Failed to downcast to Self");"#,
                    par.name, pos));
            } else {
                let type_name = match crate::analysis::rust_type::rust_type_full(
                    env,
                    par.typ,
                    par.nullable,
                    crate::analysis::ref_mode::RefMode::None,
                    library::ParameterScope::None,
                    library::Concurrency::None,
                ) {
                    Ok(s) => s,
                    Err(_) => {
                        warn_main!(par.typ, "`{}`: unknown type", env.library.type_(par.typ).get_name());
                        return Chunk::Custom("// not good".to_owned());
                    }
                };
                s.push_str(&format!(r#"
        let {0}: {1} = values[{2}]
            .get_some::<{1}>()
            .expect("Wrong argument for closure argument number {2}");"#,
                    par.name, type_name, pos));
            }
            parameter_strs.push(format!("&{}", par.name));
        }
        s.push_str(&format!("\n        f({});", parameter_strs.join(", ")));
        s.push_str("\n        None");
        s.push_str("\n    }).expect(\"connect_local failed\")");
        Chunk::Custom(s)
    } else {
        Chunk::Custom("// not good".to_owned())
    }
}
