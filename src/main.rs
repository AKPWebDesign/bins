#![feature(stmt_expr_attributes)]

extern crate clap;
#[cfg(feature = "clipboard_support")]
extern crate clipboard;
#[macro_use]
extern crate cfg_if;
#[macro_use]
extern crate error_chain;
extern crate hyper;
#[macro_use]
extern crate lazy_static;
extern crate linked_hash_map;
#[cfg(feature = "file_type_checking")]
extern crate magic_sys;
extern crate rand;
extern crate rustc_serialize;
extern crate toml;
extern crate url;

mod bins;

use bins::error::*;
use bins::arguments;
use bins::Bins;
use bins::configuration::BinsConfiguration;
#[cfg(feature = "clipboard_support")]
use clipboard::ClipboardContext;
use std::io::Write;

macro_rules! println_stderr {
  ($fmt:expr) => { { writeln!(std::io::stderr(), $fmt).expect("error writing to stderr"); } };
  ($fmt:expr, $($arg:tt)*) => { { writeln!(std::io::stderr(), $fmt, $($arg)*).expect("error writing to stderr"); } };
}

macro_rules! or_exit {
  ($expr: expr) => {
    match $expr { Ok(x) => x, Err(e) => { for err in e.iter() { println_stderr!("{}", err); } return 1; } }
  };
}

fn make_bins() -> Result<Bins> {
  let config = try!(BinsConfiguration::new());
  let arguments = try!(arguments::get_arguments(&config));
  Ok(Bins::new(config, arguments))
}

#[cfg(feature = "clipboard_support")]
fn copy_to_clipboard(data: &str) -> Result<()> {
  let mut clipboard = try!(ClipboardContext::new().map_err(|e| e.to_string()));
  clipboard.set_contents((*data).to_owned()).map_err(|e| e.to_string().into())
}

#[cfg(not(feature = "clipboard_support"))]
fn copy_to_clipboard(_: &str) -> Result<()> {
  Ok(())
}

fn inner() -> i32 {
  let bins = or_exit!(make_bins());
  let output = or_exit!(bins.get_output());
  if bins.arguments.copy {
    or_exit!(copy_to_clipboard(&output));
  }
  println!("{}", output);
  0
}

fn main() {
  let exit_code = inner();
  std::process::exit(exit_code);
}
