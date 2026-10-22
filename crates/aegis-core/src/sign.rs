use hmac::{Hmac, Mac};
use sha2::Sha256;

pub(crate) fn hmac_sha256_hex(secret: &str, payload: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC accepts keys of any length");
    mac.update(payload.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Application clock (computer clock plus the measured correction), used for request
/// signatures and quote freshness.
pub(crate) fn now_ms() -> i64 {
    crate::clock::now_ms()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_binance_documentation_vector() {
        // Example from the Binance API docs ("SIGNED endpoint examples").
        let secret = "NhqPtmdSJYdKjVHjA7PZj4Mge3R5YNiP1e3UZjInClVN65XAbvqqM6A7H5fATj0j";
        let query = "symbol=LTCBTC&side=BUY&type=LIMIT&timeInForce=GTC&quantity=1&price=0.1&recvWindow=5000&timestamp=1499827319559";
        assert_eq!(
            hmac_sha256_hex(secret, query),
            "c8db56825ae71d6d79447849e617115f4a920fa2acdcab2b053c4b2838bd6b71"
        );
    }
}
