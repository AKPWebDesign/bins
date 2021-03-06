pub mod bitbucket;
pub mod gist;
pub mod hastebin;
pub mod pastebin;
pub mod pastie;
pub mod sprunge;

use bins::error::*;
use bins::network::download::Downloader;
use bins::network::upload::Uploader;
use bins::network;
use bins::{self, Bins, PasteFile};
use hyper::Url;
use linked_hash_map::LinkedHashMap;
use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::Write;
use std::iter::repeat;
use std::path::{Path, PathBuf};
use rustc_serialize::{Encodable, Encoder};

#[derive(Debug)]
pub struct Index {
  pub files: LinkedHashMap<String, Url>
}

impl Index {
  fn repeat_str(string: &str, count: usize) -> String {
    repeat(string).take(count).collect()
  }

  pub fn to_string(&self) -> String {
    let header = format!("{} files", self.files.len());
    let separator = Self::repeat_str("-", header.len());
    let mut body = String::from("");
    for (i, (name, url)) in self.files.iter().enumerate() {
      let number = i + 1;
      body.push_str(&format!("{number}. {name}: {url}\n",
                             number = number,
                             name = name,
                             url = url));
    }
    format!("{}\n{}\n\n{}", header, separator, body)
  }

  pub fn parse<S: Into<String>>(string: S) -> Result<Index> {
    let string = string.into();
    let lines: Vec<&str> = string.split('\n').collect();
    if lines.len() < 4 {
      return Err(ErrorKind::InvalidIndexError.into());
    }
    let mut split = lines.iter().skip(3).filter(|s| !s.trim().is_empty()).map(|s| s.split(": ")).collect::<Vec<_>>();
    let names: Vec<String> = some_or_err!(split.iter_mut()
                                            .map(|s| {
                                              s.nth(0)
                                                .map(|n| n.split(' ').skip(1).collect::<Vec<&str>>().join(" "))
                                            })
                                            .collect(),
                                          ErrorKind::InvalidIndexError.into());
    let url_strings: Vec<String> = some_or_err!(split.iter_mut().map(|s| s.nth(0).map(|s| s.to_owned())).collect(),
                                                ErrorKind::InvalidIndexError.into());
    if url_strings.is_empty() {
      return Err(ErrorKind::InvalidIndexError.into());
    }
    let urls: Result<Vec<Url>> = url_strings.into_iter().map(network::parse_url).collect();
    let urls: Vec<Url> = try!(urls.chain_err(|| ErrorKind::InvalidIndexError));
    let urls: LinkedHashMap<String, Url> = names.into_iter().zip(urls.into_iter()).collect();
    Ok(Index { files: urls })
  }
}

#[derive(Debug, Clone, RustcEncodable)]
pub struct PasteContents {
  pub truncated: bool,
  pub value: Option<String>
}

impl Default for PasteContents {
  fn default() -> Self {
    PasteContents {
      truncated: false,
      value: None
    }
  }
}

#[derive(Debug)]
pub struct Info {
  name: String,
  id: String,
  url: Url,
  raw_url: Option<Url>,
  raw: bool,
  files: Vec<RemotePasteFile>,
  index: Option<Index>,
  contents: PasteContents,
  bin: String
}

#[derive(Debug, Clone)]
pub struct RemotePasteFile {
  pub id: String,
  pub name: String,
  pub bin: String,
  pub url: Url,
  pub raw_url: Url,
  pub contents: PasteContents
}

impl Encodable for RemotePasteFile {
  fn encode<S: Encoder>(&self, s: &mut S) -> ::std::result::Result<(), S::Error> {
    s.emit_struct("RemotePasteFile", 6, |s| {
      try!(s.emit_struct_field("id", 0, |s| self.id.encode(s)));
      try!(s.emit_struct_field("name", 1, |s| self.name.encode(s)));
      try!(s.emit_struct_field("bin", 2, |s| self.bin.encode(s)));
      try!(s.emit_struct_field("url", 3, |s| s.emit_str(self.url.as_str())));
      try!(s.emit_struct_field("raw_url", 4, |s| s.emit_str(self.raw_url.as_str())));
      try!(s.emit_struct_field("contents", 5, |s| self.contents.encode(s)));
      Ok(())
    })
  }
}

pub trait ProduceId {
  fn produce_id(&self, bins: &Bins, url: &Url) -> Result<String>;
}

impl<T> ProduceId for T
  where T: Bin
{
  fn produce_id(&self, _: &Bins, url: &Url) -> Result<String> {
    Ok(some_or_err!(url.path_segments().and_then(|s| s.last()).map(|s| s.to_owned()),
                    "no last path segment".into()))
  }
}

/// Produce information about HTML content from URLs to HTML content.
pub trait ProduceInfo {
  fn produce_info(&self, bins: &Bins, url: &Url) -> Result<Info>;

  fn produce_info_all(&self, bins: &Bins, urls: Vec<&Url>) -> Result<Vec<Info>> {
    let info: Vec<Info> = try!(urls.iter().map(|u| self.produce_info(bins, u)).collect());
    Ok(info)
  }
}

impl<T> ProduceInfo for T
  where T: GenerateIndex + ConvertUrlsToRawUrls + Downloader + Bin
{
  fn produce_info(&self, bins: &Bins, url: &Url) -> Result<Info> {
    let raw_url = try!(self.convert_url_to_raw_url(bins, url));
    let mut res = try!(self.download(&bins, &raw_url));
    let content = try!(network::read_response(&mut res));
    let index = Index::parse(content.clone());
    let mut urls: Vec<RemotePasteFile> = Vec::new();
    match index {
      Ok(ref i) => {
        for (name, url) in i.files.clone().into_iter() {
          urls.push(RemotePasteFile {
            name: name.clone(),
            id: try!(self.produce_id(bins, &url)),
            bin: self.get_name().to_owned(),
            url: url.clone(),
            raw_url: try!(self.convert_url_to_raw_url(bins, &url)),
            contents: PasteContents::default()
          });
        }
      }
      Err(ref e) => {
        if let ErrorKind::InvalidIndexError = *e.kind() {} else {
          return Err(e.to_string().into());
        }
        let url = url.clone();
        let name = some_or_err!(url.path_segments().and_then(|s| s.last()),
                                "paste url was a root url".into());
        urls.push(RemotePasteFile {
          name: name.to_owned(),
          id: name.to_owned(),
          bin: self.get_name().to_owned(),
          url: url.clone(),
          raw_url: raw_url.clone(),
          contents: PasteContents {
            truncated: false,
            value: Some(content.clone())
          }
        });
      }
    }
    let id = try!(self.produce_id(bins, &url));
    Ok(Info {
      id: id.clone(),
      name: id,
      url: url.clone(),
      raw_url: Some(raw_url),
      raw: false,
      files: urls,
      index: index.ok(),
      contents: PasteContents {
        truncated: false,
        value: Some(content)
      },
      bin: self.get_name().to_owned()
    })
  }
}

/// Produce information about raw content from URLs to HTML content.
pub trait ProduceRawInfo {
  fn produce_raw_info(&self, bins: &Bins, url: &Url) -> Result<Info>;

  fn produce_raw_info_all(&self, bins: &Bins, urls: Vec<&Url>) -> Result<Vec<Info>> {
    let info: Vec<Info> = try!(urls.iter().map(|u| self.produce_raw_info(bins, u)).collect());
    Ok(info)
  }
}

impl<T> ProduceRawInfo for T
  where T: ConvertUrlsToRawUrls + Downloader + UsesIndices + ProduceInfo
{
  fn produce_raw_info(&self, bins: &Bins, url: &Url) -> Result<Info> {
    let mut info = try!(self.produce_info(bins, url));
    info.raw = true;
    Ok(info)
  }
}

pub trait ConvertUrlsToRawUrls {
  fn convert_url_to_raw_url(&self, bins: &Bins, url: &Url) -> Result<Url>;

  fn convert_urls_to_raw_urls(&self, bins: &Bins, urls: Vec<&Url>) -> Result<Vec<Url>> {
    urls.iter().map(|u| self.convert_url_to_raw_url(bins, u)).collect()
  }
}

#[derive(RustcEncodable)]
struct JsonOutput<'a> {
  id: &'a str,
  name: &'a str,
  bin: &'a str,
  url: &'a str,
  raw_url: Option<&'a str>,
  contents: &'a PasteContents,
  files: Option<&'a [RemotePasteFile]>
}

/// Produce raw content from a URL to HTML content.
pub trait ProduceRawContent: ProduceRawInfo + ProduceInfo + Downloader {
  fn produce_raw_contents(&self, bins: &Bins, url: &Url) -> Result<String> {
    let info = if bins.arguments.urls {
      try!(self.produce_info(bins, url))
    } else {
      try!(self.produce_raw_info(bins, url))
    };
    let raw_files = info.files.clone();
    let raw_files: Vec<RemotePasteFile> = if raw_files.len() > 1 {
      if !bins.arguments.files.is_empty() {
        let mut map: HashMap<String, RemotePasteFile> =
          raw_files.into_iter().map(|r| (r.name.to_lowercase(), r)).collect();
        try!(bins.arguments
          .files
          .iter()
          .map(|s| map.remove(&s.to_lowercase()).ok_or(format!("file {} not found", s)))
          .collect())
      } else if let Some(ref range) = bins.arguments.range {
        let mut numbered_info: HashMap<usize, RemotePasteFile> = raw_files.into_iter()
          .enumerate()
          .collect();
        try!(range.clone()
          .into_iter()
          .map(|n| numbered_info.remove(&n).ok_or(format!("file {} not found", n)))
          .collect())
      } else if bins.arguments.all {
        raw_files
      } else {
        let names = raw_files.into_iter().map(|r| String::from("  ") + &r.name).collect::<Vec<_>>().join("\n");
        return Err(format!("paste had multiple files, but no behavior was specified on the command \
                            line\n\navailable files:\n{}",
                           names)
          .into());
      }
    } else {
      raw_files
    };
    if bins.arguments.raw_urls || bins.arguments.urls {
      return Ok(raw_files.into_iter().map(|r| r.url.as_str().to_owned()).collect::<Vec<_>>().join("\n"));
    }
    let names: Vec<String> = raw_files.iter().map(|p| p.name.clone()).collect();
    let all_contents: Vec<String> = try!(raw_files.iter()
      .map(|p| {
        let contents = if p.contents.truncated {
          None
        } else {
          p.contents.value.clone()
        };
        match contents {
          Some(contents) => Ok(contents),
          None => self.download(&bins, &p.raw_url).and_then(|mut r| network::read_response(&mut r)),
        }
      })
      .collect());
    let files: LinkedHashMap<String, String> = names.into_iter().zip(all_contents.into_iter()).collect();
    if bins.arguments.json {
      // JSON output should display all known files but only have content for specified files
      let mut raw_files = info.files;
      if raw_files.len() < 1 {
        return Err("paste had no output (this is a bug)".into());
      }
      for mut r in &mut raw_files {
        r.contents.value = files.get(&r.name).cloned();
      }
      let json = JsonOutput {
        name: &info.name,
        id: &info.id,
        bin: &info.bin,
        url: info.url.as_str(),
        raw_url: info.raw_url.as_ref().map(|u| u.as_str()),
        contents: &info.contents,
        files: if raw_files.len() < 2 {
          None
        } else {
          Some(&raw_files)
        }
      };
      return Ok(try!(::rustc_serialize::json::encode(&json)));
    }
    let paste_files = files.into_iter()
      .map(|(name, content)| {
        PasteFile {
          name: name.clone(),
          data: if bins.arguments.number_lines {
            number_lines(content.clone())
          } else {
            content.clone()
          }
        }
      })
      .collect::<Vec<PasteFile>>();
    if bins.arguments.write {
      let mut bins_output = String::new();
      let output = match bins.arguments.output {
        Some(ref s) => PathBuf::from(s),
        None => try!(env::current_dir()),
      };
      if !output.exists() {
        return Err("output dir did not exist".into());
      }
      if !output.is_dir() || output.is_file() {
        return Err("output dir was not a directory".into());
      }
      for p in &paste_files {
        let sanitized = try!(Bins::sanitize_path(Path::new(&p.name)));
        let original_path = output.join(sanitized);
        let mut path = original_path.clone();
        let mut num = 0;
        while path.exists() {
          num += 1;
          path = Bins::add_number_to_path(&original_path, num);
        }
        let mut file = try!(File::create(&path));
        try!(file.write_all(p.data.as_bytes()));
        bins_output.push_str(format!("Wrote {} -> {}\n", p.name, path.to_string_lossy()).as_str());
      }
      return Ok(bins_output[0..bins_output.len() - 1].to_owned());
    }
    Ok(paste_files.join())
  }
}

fn number_lines(string: String) -> String {
  let lines: Vec<&str> = string.split('\n').collect();
  let num_lines = lines.len();
  let zeroes: String = repeat(" ").take(num_lines.to_string().len()).collect();
  lines.into_iter()
    .enumerate()
    .map(|(i, l)| {
      let i = i + 1;
      format!("{}{}  {}",
              &zeroes[0..zeroes.len() - i.to_string().len()],
              i,
              l)
    })
    .collect::<Vec<_>>()
    .join("\n")
}

/// Produce a URL to HTML content from raw content.
pub trait UploadContent: Uploader {
  fn upload_paste(&self, bins: &Bins, content: PasteFile) -> Result<Url>;
}

impl<T> UploadContent for T
  where T: UploadUrl + Uploader
{
  fn upload_paste(&self, bins: &Bins, content: PasteFile) -> Result<Url> {
    let url = try!(network::parse_url(self.get_upload_url()));
    let mut response = try!(self.upload(&url, bins, &content));
    if response.status.class().default_code() != ::hyper::Ok {
      return Err(format!("status code {}", response.status).into())
    }
    network::parse_url(try!(network::read_response(&mut response)))
  }
}

/// Produce a URL to HTML content from a batch of raw content.
pub trait UploadBatchContent: UploadContent {
  fn upload_all(&self, bins: &Bins, content: Vec<PasteFile>) -> Result<Url>;
}

impl<T> UploadBatchContent for T
  where T: GenerateIndex + UploadContent
{
  fn upload_all(&self, bins: &Bins, content: Vec<PasteFile>) -> Result<Url> {
    let index = try!(self.generate_index(bins, content));
    self.upload_paste(bins,
                      PasteFile {
                        name: "index.md".to_owned(),
                        data: index.to_string()
                      })
  }
}

pub trait UsesIndices {}

/// Generate an index for multiple files.
pub trait GenerateIndex {
  fn generate_index(&self, bins: &Bins, content: Vec<PasteFile>) -> Result<Index>;
}

impl<T> GenerateIndex for T
  where T: UploadContent + UsesIndices
{
  fn generate_index(&self, bins: &Bins, content: Vec<PasteFile>) -> Result<Index> {
    let names: Vec<String> = (&content).into_iter().map(|p| p.name.clone()).collect();
    let urls: Vec<Url> = try!(content.into_iter().map(|p| self.upload_paste(bins, p)).collect());
    let uploads: LinkedHashMap<String, Url> = names.into_iter().zip(urls.into_iter()).collect();
    Ok(Index { files: uploads })
  }
}

pub trait UploadUrl {
  fn get_upload_url(&self) -> &str;
}

pub trait VerifyUrl {
  fn segments(&self, url: &Url) -> Vec<String> {
    let segments = match url.path_segments() {
      Some(s) => s,
      None => return Vec::new(),
    };
    segments.filter(|s| !s.is_empty()).map(|s| s.to_owned()).collect::<Vec<_>>()
  }

  fn verify_url(&self, url: &Url) -> bool;
}

/// A bin, which can upload content in raw form and download content in raw and HTML form.
pub trait Bin: Sync + ProduceInfo + ProduceRawContent + UploadBatchContent + VerifyUrl {
  fn get_name(&self) -> &str;

  fn get_domain(&self) -> &str;
}

trait Join {
  fn join(&self) -> String;
}

impl Join for Vec<PasteFile> {
  fn join(&self) -> String {
    if self.len() == 1 {
      self.get(0).expect("len() == 1, but no first element").data.clone()
    } else {
      self.into_iter().map(|p| format!("==> {} <==\n{}", p.name, p.data)).collect::<Vec<String>>().join("\n")
    }
  }
}

lazy_static! {
  pub static ref BINS: Vec<Box<Bin>> = {
      vec![
        Box::new(bins::Gist::new()),
        Box::new(bins::Bitbucket::new()),
        Box::new(bins::Sprunge::new()),
        Box::new(bins::Hastebin::new()),
        Box::new(bins::Pastebin::new()),
        Box::new(bins::Pastie::new())
      ]
  };
}

pub fn get_bin_names<'a>() -> Vec<&'a str> {
  BINS.iter().map(|e| e.get_name()).collect()
}

pub fn get_bin_by_name(name: &str) -> Option<&Box<Bin>> {
  BINS.iter().find(|e| e.get_name().to_lowercase() == name.to_lowercase())
}

pub fn get_bin_by_domain(domain: &str) -> Option<&Box<Bin>> {
  BINS.iter().find(|e| e.get_domain().to_lowercase() == domain.to_lowercase())
}
