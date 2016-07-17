extern crate git2;
extern crate rustc_version;

use git2::{DescribeFormatOptions, DescribeOptions, Repository};
use rustc_version::version_matches;
use std::env;
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
use std::process::exit;

fn get_version() -> String {
  let profile = env::var("PROFILE").unwrap();
  let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
  let mut info = Vec::new();
  info.push(format!("profile: {}", profile));
  if let Ok(repo) = Repository::open(&manifest_dir) {
    let version = repo.describe(DescribeOptions::new().describe_tags().show_commit_oid_as_fallback(true))
      .unwrap()
      .format(Some(DescribeFormatOptions::new().dirty_suffix("-dirty")))
      .unwrap();
    info.push(format!("git: {}", version));
  };
  info.join("\n")
}

fn main() {
  if !version_matches(">= 1.10.0") {
    writeln!(&mut io::stderr(), "bins requires at least Rust 1.10.0").unwrap();
    exit(1);
  }
  let version = get_version();
  let out_dir = env::var("OUT_DIR").unwrap();
  let dest_path = Path::new(&out_dir).join("extra_version_info.rs");
  let mut f = File::create(&dest_path).unwrap();
  f.write_all(format!("
      fn extra_version_info() -> &'static str {{
          \"{}\"
      }}
  ",
                       version)
      .as_bytes())
    .unwrap();
  if cfg!(feature = "copypasta") {
    let dest_path = Path::new(&out_dir).join("copypasta.rs");
    let mut f = File::create(&dest_path).unwrap();
    f.write_all(r#"
        fn dank_memes() -> &'static str {
          "What the fuck did you just fucking say about me, you little bitch? I’ll have you know I graduated top of my class in the Navy Seals, and I’ve been involved in numerous secret raids on Al-Quaeda, and I have over 300 confirmed kills. I am trained in gorilla warfare and I’m the top sniper in the entire US armed forces. You are nothing to me but just another target. I will wipe you the fuck out with precision the likes of which has never been seen before on this Earth, mark my fucking words. You think you can get away with saying that shit to me over the Internet? Think again, fucker. As we speak I am contacting my secret network of spies across the USA and your IP is being traced right now so you better prepare for the storm, maggot. The storm that wipes out the pathetic little thing you call your life. You’re fucking dead, kid. I can be anywhere, anytime, and I can kill you in over seven hundred ways, and that’s just with my bare hands. Not only am I extensively trained in unarmed combat, but I have access to the entire arsenal of the United States Marine Corps and I will use it to its full extent to wipe your miserable ass off the face of the continent, you little shit. If only you could have known what unholy retribution your little “clever” comment was about to bring down upon you, maybe you would have held your fucking tongue. But you couldn’t, you didn’t, and now you’re paying the price, you goddamn idiot. I will shit fury all over you and you will drown in it. You’re fucking dead, kiddo."
        }
    "#.as_bytes()).unwrap()
  }
}
