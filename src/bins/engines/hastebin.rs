use bins::error::*;
use bins::engines::{Bin, ConvertUrlsToRawUrls, ProduceRawContent, UploadContent, UsesIndices, VerifyUrl};
use bins::network::download::{Downloader, ModifyDownloadRequest};
use bins::network::upload::{ModifyUploadRequest, Uploader};
use bins::network::{self, RequestModifiers};
use bins::{Bins, PasteFile};
use hyper::Url;
use rustc_serialize::json;

pub struct Hastebin;

impl Hastebin {
  pub fn new() -> Self {
    Hastebin {}
  }

  fn get_url(&self, bins: &Bins) -> Result<Url> {
    if let Some(ref url) = bins.arguments.server {
      let mut url = url.clone();
      url.set_path("");
      Ok(url)
    } else {
      network::parse_url(bins.config.get_hastebin_server().unwrap_or("https://hastebin.com"))
    }
  }

  fn get_upload_url(&self, bins: &Bins) -> Result<Url> {
    let mut url = try!(self.get_url(bins));
    url.set_path("/documents");
    Ok(url)
  }
}

impl Bin for Hastebin {
  fn get_name(&self) -> &str {
    "hastebin"
  }

  fn get_domain(&self) -> &str {
    "hastebin.com"
  }
}

impl VerifyUrl for Hastebin {
  fn verify_url(&self, url: &Url) -> bool {
    self.segments(url).len() == 1
  }
}

#[derive(Debug, RustcDecodable)]
struct HastebinResponse {
  key: String
}

impl UploadContent for Hastebin {
  fn upload_paste(&self, bins: &Bins, content: PasteFile) -> Result<Url> {
    let url = try!(self.get_upload_url(bins));
    let mut response = try!(self.upload(&url, bins, &content));
    if response.status.class().default_code() != ::hyper::Ok {
      return Err(format!("status code {}", response.status).into());
    }
    let json = try!(network::read_response(&mut response));
    let content: HastebinResponse = try!(json::decode(&json));
    let scheme = url.scheme();
    let domain = some_or_err!(url.domain(), "response had no domain".into());
    Ok(try!(network::parse_url(format!("{}://{}/{}", scheme, domain, content.key))))
  }
}

impl ConvertUrlsToRawUrls for Hastebin {
  fn convert_url_to_raw_url(&self, _: &Bins, url: &Url) -> Result<Url> {
    let mut u = url.clone();
    let name = {
      let segments = some_or_err!(u.path_segments().and_then(|s| s.last()),
                                  "url was root url".into());
      let parts = segments.split('.').collect::<Vec<_>>();
      if parts.len() > 1 {
        parts[..parts.len() - 1].join(".")
      } else {
        parts[0].to_owned()
      }
    };
    u.set_path(format!("/raw/{}", name).as_str());
    Ok(u)
  }
}

impl ModifyUploadRequest for Hastebin {
  fn modify_request<'a>(&'a self, _: &Bins, content: &PasteFile) -> Result<RequestModifiers> {
    Ok(RequestModifiers { body: Some(content.data.clone()), ..RequestModifiers::default() })
  }
}

unsafe impl Sync for Hastebin {}

impl UsesIndices for Hastebin {}

impl ProduceRawContent for Hastebin {}

impl Uploader for Hastebin {}

impl Downloader for Hastebin {}

impl ModifyDownloadRequest for Hastebin {}
