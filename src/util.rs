use libaes::Cipher;
use std::env;

pub fn new_reqwest_client() -> reqwest::Client {
    let mut builder = reqwest::Client::builder();
    if let Ok(http_proxy) = env::var("HTTP_PROXY") {
        builder = builder.proxy(reqwest::Proxy::http(http_proxy).unwrap());
    }
    if let Ok(https_proxy) = env::var("HTTPS_PROXY") {
        builder = builder.proxy(reqwest::Proxy::https(https_proxy).unwrap());
    }
    builder = builder.user_agent("reqwest/0.11.17");
    builder.build().unwrap()
}

pub fn decrypt(data: &[u8], secret: &[u8]) -> String {
    let key = &secret[0..32];
    let iv = &secret[32..(32 + 16)];
    let cipher = Cipher::new_256(key.try_into().unwrap());
    String::from_utf8(cipher.cbc_decrypt(iv, data)).unwrap()
}
