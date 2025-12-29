#[cfg(all(test, feature = "aws-kms"))]
mod tests {
    use mev_arbitrage_bot::kms::aws::AwsKmsClient as Client;
    use openssl::ec::{EcKey, EcGroup};
    use openssl::nid::Nid;
    use openssl::pkey::PKey;

    #[test]
    fn parse_spki_from_secp256k1_succeeds() {
        // generate a secp256k1 key and export SubjectPublicKeyInfo DER
        use openssl::ec::EcGroup;
        let group = EcGroup::from_curve_name(Nid::SECP256K1).unwrap();
        let ec = EcKey::generate(&group).unwrap();
        let pkey = PKey::from_ec_key(ec).unwrap();
        let der = pkey.public_key_to_der().unwrap();
        let addr = Client::parse_spki_to_address(&der).expect("parse should succeed");
        // basic sanity: addr should be 20 bytes and non-zero
        assert_ne!(addr, ethers_core::types::Address::zero());
    }

    #[test]
    fn parse_spki_wrong_curve_fails() {
        // generate a prime256v1 (P-256) key and export DER
        let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap();
        let ec = EcKey::generate(&group).unwrap();
        let pkey = PKey::from_ec_key(ec).unwrap();
        let der = pkey.public_key_to_der().unwrap();
        let res = Client::parse_spki_to_address(&der);
        assert!(res.is_err());
    }

    #[test]
    fn parse_spki_invalid_der_fails() {
        let res = Client::parse_spki_to_address(b"not-a-der");
        assert!(res.is_err());
    }
}
