use std::collections::HashSet;
use std::fmt::Display;
use std::io::{Result, Write};

use analysis::general::StatusedTypeId;
use config::Config;
use git::repo_hash;
use gir_version::VERSION;
use nameutil::crate_name;
use version::Version;

pub fn start_comments<W: Write>(w: &mut W, conf: &Config) -> Result<()>{
    try!(writeln!(w, "// This file was generated by gir ({}) from gir-files ({})",
            VERSION, repo_hash(&conf.girs_dir).unwrap_or_else(|_| "???".into())));
    try!(writeln!(w, "// DO NOT EDIT"));

    Ok(())
}

pub fn uses<W: Write>(w: &mut W, used_types: &HashSet<String>) -> Result<()>{
    let v = vec![
        "",
        "use glib::translate::*;",
        "use glib::types;",
        "use ffi;",
        "",
        "use object::*;",
    ];
    try!(write_vec(w, &v));

    let mut used_types: Vec<String> = used_types.iter()
        .map(|s| s.clone()).collect();
    used_types.sort_by(|a, b| a.cmp(b));

    for name in used_types {
        try!(writeln!(w, "use {};", name));
    }

    Ok(())
}

pub fn objects_child_type<W: Write>(w: &mut W, type_name: &str, glib_name: &str) -> Result<()>{
    try!(writeln!(w, ""));
    try!(writeln!(w, "pub type {} = Object<ffi::{}>;", type_name, glib_name));

    Ok(())
}

pub fn impl_parents<W: Write>(w: &mut W, type_name: &str, parents: &[StatusedTypeId]) -> Result<()>{
    try!(writeln!(w, ""));
    for stid in parents {
        //TODO: don't generate for parents without traits
        try!(writeln!(w, "unsafe impl Upcast<{}> for {} {{ }}", stid.name, type_name));
    }

    Ok(())
}

pub fn impl_interfaces<W: Write>(w: &mut W, type_name: &str, implements: &[StatusedTypeId]) -> Result<()>{
    for stid in implements {
        try!(writeln!(w, "unsafe impl Upcast<{}> for {} {{ }}", stid.name, type_name));
    }

    Ok(())
}

pub fn impl_static_type<W: Write>(w: &mut W, type_name: &str, glib_func_name: &str) -> Result<()>{
    try!(writeln!(w, ""));
    try!(writeln!(w, "impl types::StaticType for {} {{", type_name));
    try!(writeln!(w, "{}#[inline]", tabs(1)));
    try!(writeln!(w, "{}fn static_type() -> types::Type {{", tabs(1)));
    try!(writeln!(w, "{}unsafe {{ from_glib(ffi::{}()) }}", tabs(2), glib_func_name));
    try!(writeln!(w, "{}}}", tabs(1)));
    try!(writeln!(w, "}}"));

    Ok(())
}

pub fn version_condition<W: Write>(w: &mut W, library_name: &str, min_cfg_version: Version,
        version: Option<Version>, commented: bool, indent: i32) -> Result<()> {
    let s = version_condition_string(library_name, min_cfg_version, version, commented, indent);
    if let Some(s) = s {
        try!(writeln!(w, "{}", s));
    }
    Ok(())
}

pub fn version_condition_string(library_name: &str, min_cfg_version: Version,
        version: Option<Version>, commented: bool, indent: i32) -> Option<String> {
    match version {
        Some(v) if v >= min_cfg_version => {
            let comment = if commented { "//" } else { "" };
            Some(format!("{}{}#[cfg({})]", tabs(indent), comment,
                v.to_cfg(&crate_name(library_name))))
        }
        _ => None
    }
}

//TODO: convert to macro with usage
//format!(indent!(5, "format:{}"), 6)
pub fn tabs(num: i32) -> String {
    (0..num).map(|_| "    ").collect::<String>()
}

pub fn write_vec<W: Write, T: Display>(w: &mut W, v: &[T]) -> Result<()> {
    for s in v {
        try!(writeln!(w, "{}", s));
    }
    Ok(())
}
