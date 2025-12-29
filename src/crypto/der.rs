use anyhow::{Result, anyhow};
use k256::ecdsa::Signature as KSignature;
use secp256k1::{Secp256k1, ecdsa::{RecoverableSignature, RecoveryId}, Message as SecpMessage};
use ethers_core::types::{Signature, Address};

/// Convert a DER-encoded ECDSA signature (as returned by many KMS providers) into
/// an `ethers::types::Signature` (r, s, v) by attempting public-key recovery
/// over the four possible recovery ids and comparing against an optional expected address.
pub fn der_to_ethers_signature(der_sig: &[u8], msg_hash: &[u8], expected_address: Option<Address>) -> Result<Signature> {
    // Parse DER signature (ASN.1) using k256
    let ksig = KSignature::from_der(der_sig).map_err(|e| anyhow!("invalid der signature: {}", e))?;
    let compact = ksig.to_bytes(); // 64 bytes: r||s

    if msg_hash.len() != 32 {
        return Err(anyhow!("message hash must be 32 bytes"));
    }

    // Prepare message and use secp256k1 recoverable API to recover the verifying key for each possible id
    let msg = SecpMessage::from_slice(msg_hash).map_err(|e| anyhow::anyhow!("{}", e))?;
    let secp = Secp256k1::new();
    // Curve order for secp256k1
    let curve_n = ethers_core::types::U256::from_big_endian(&hex::decode("FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141").unwrap());
    let half_n = curve_n.checked_div(ethers_core::types::U256::from(2u64)).unwrap();

    for recid_val in 0..4 {
        let recid = RecoveryId::from_i32(recid_val).map_err(|e| anyhow::anyhow!("{}", e))?;
        let rec_sig = RecoverableSignature::from_compact(&compact, recid).map_err(|e| anyhow::anyhow!("{}", e))?;
        if let Ok(pk) = secp.recover_ecdsa(&msg, &rec_sig) {
            let serialized = pk.serialize_uncompressed();
            let pubkey_bytes = &serialized[1..65];
            let addr_bytes = ethers_core::utils::keccak256(pubkey_bytes);
            let addr = Address::from_slice(&addr_bytes[12..]);
            if expected_address.is_none() || expected_address.unwrap() == addr {
                let mut r = ethers_core::types::U256::from_big_endian(&compact[0..32]);
                let mut s = ethers_core::types::U256::from_big_endian(&compact[32..64]);
                let mut v = (recid_val as u64) + 27u64;
                // Enforce low-s canonical form: if s > N/2, set s = N - s and flip v
                if s > half_n {
                    s = curve_n.checked_sub(s).unwrap_or_default();
                    v = if v == 27 { 28u64 } else { 27u64 };
                }
                let sig = Signature { r, s, v };
                return Ok(sig);
            }
        }
    }

    Err(anyhow!("unable to recover public key from signature"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use secp256k1::{Secp256k1, SecretKey, PublicKey, Message as SecpMessage};
    use ethers_core::types::Address;
    use rand::rngs::OsRng;

    #[test]
    fn der_roundtrip_recoverable() {
        let secp = Secp256k1::new();
        let mut rng = OsRng;
        let mut sk_bytes = [0u8; 32];
        use rand::RngCore;
        rng.fill_bytes(&mut sk_bytes);
        let sk = SecretKey::from_slice(&sk_bytes).expect("secret");
        let pk = PublicKey::from_secret_key(&secp, &sk);
        let serialized = pk.serialize_uncompressed();
        let pubkey_bytes = &serialized[1..65];
        let addr_bytes = ethers_core::utils::keccak256(pubkey_bytes);
        let addr = Address::from_slice(&addr_bytes[12..]);

        // sign a 32-byte message hash
        let msg_hash = ethers_core::utils::keccak256(b"hello-der-test");
        let msg = SecpMessage::from_slice(&msg_hash).unwrap();
        let recsig = secp.sign_ecdsa_recoverable(&msg, &sk);

        // convert to standard (r,s) and then to DER
        let stdsig = recsig.to_standard();
        let der = stdsig.serialize_der().to_vec();

        let sig = der_to_ethers_signature(&der, &msg_hash, Some(addr)).expect("recovery");
        assert!(sig.r != ethers_core::types::U256::zero());
        assert!(sig.s != ethers_core::types::U256::zero());
        assert!(sig.v == 27 || sig.v == 28);
    }
}
