use ethers_core::types::{Address, Bytes, NameOrAddress, U256};
use ethers_core::types::transaction::eip1559::Eip1559TransactionRequest;
use ethers_core::types::transaction::eip2718::TypedTransaction;
use anyhow::Result;

/// Build a basic EIP-1559 `TypedTransaction`.
pub fn build_eip1559_tx(
    nonce: U256,
    to: Address,
    value: U256,
    data: Bytes,
    gas_limit: U256,
    max_priority_fee_per_gas: U256,
    max_fee_per_gas: U256,
    chain_id: u64,
) -> TypedTransaction {
    let mut tx = Eip1559TransactionRequest::new();
    tx = tx.nonce(nonce);
    tx = tx.to(NameOrAddress::Address(to));
    tx = tx.value(value);
    tx = tx.data(data);
    tx = tx.gas(gas_limit);
    tx = tx.max_priority_fee_per_gas(max_priority_fee_per_gas);
    tx = tx.max_fee_per_gas(max_fee_per_gas);
    tx = tx.chain_id(chain_id);

    TypedTransaction::Eip1559(tx)
}

/// Given a list of signed raw tx bytes, produce a JSON array suitable for a
/// Flashbots-style bundle submission (array of hex strings prefixed with 0x).
pub fn bundle_from_signed_txs(signed: &[Vec<u8>]) -> serde_json::Value {
    let arr: Vec<String> = signed.iter().map(|s| format!("0x{}", hex::encode(s))).collect();
    serde_json::Value::Array(arr.into_iter().map(serde_json::Value::String).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ethers_core::types::{Address, U256, Bytes};

    #[test]
    fn builds_eip1559_tx() {
        let nonce = U256::from(1u64);
        let to = Address::zero();
        let value = U256::from(0u64);
        let data = Bytes::from(vec![]);
        let gas_limit = U256::from(21000u64);
        let max_priority = U256::from(1_000_000_000u64);
        let max_fee = U256::from(100_000_000_000u64);
        let tx = build_eip1559_tx(nonce, to, value, data, gas_limit, max_priority, max_fee, 1);
        match tx {
            TypedTransaction::Eip1559(_r) => {}
            _ => panic!("expected Eip1559 transaction"),
        }
    }

    #[test]
    fn bundle_from_tx_encodes_hex_array() {
        let signed = vec![vec![0x01, 0x02, 0x03], vec![0xab, 0xcd]];
        let v = bundle_from_signed_txs(&signed);
        assert!(v.is_array());
        let arr = v.as_array().unwrap();
        assert_eq!(arr[0].as_str().unwrap(), "0x010203");
        assert_eq!(arr[1].as_str().unwrap(), "0xabcd");
    }
}
