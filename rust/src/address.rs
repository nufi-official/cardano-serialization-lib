use super::*;
use prelude::*;
use bech32::ToBase32;

// returns (Number represented, bytes read) if valid encoding
// or None if decoding prematurely finished
fn variable_nat_decode(bytes: &[u8]) -> Option<(u64, usize)> {
    let mut output = 0u64;
    let mut bytes_read = 0;
    for byte in bytes {
        output = (output << 7) | (byte & 0x7F) as u64;
        bytes_read += 1;
        if (byte & 0x80) == 0 {
            return Some((output, bytes_read));
        }
    }
    None
}

fn variable_nat_encode(mut num: u64) -> Vec<u8> {
    let mut output = vec![num as u8 & 0x7F];
    num /= 128;
    while num > 0 {
        output.push((num & 0x7F) as u8 | 0x80);
        num /= 128;
    }
    output.reverse();
    output
}

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd)]
enum StakeCredType {
    Key(AddrKeyHash),
    Script(ScriptHash),
}

#[wasm_bindgen]
#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct StakeCredential(StakeCredType);

#[wasm_bindgen]
impl StakeCredential {
    pub fn from_keyhash(hash: &AddrKeyHash) -> Self {
        StakeCredential(StakeCredType::Key(hash.clone()))
    }

    pub fn from_scripthash(hash: &ScriptHash) -> Self {
        StakeCredential(StakeCredType::Script(hash.clone()))
    }

    pub fn to_keyhash(&self) -> Option<AddrKeyHash> {
        match &self.0 {
            StakeCredType::Key(hash) => Some(hash.clone()),
            StakeCredType::Script(_) => None,
        }
    }

    pub fn to_scripthash(&self) -> Option<ScriptHash> {
        match &self.0 {
            StakeCredType::Key(_) => None,
            StakeCredType::Script(hash) => Some(hash.clone()),
        }
    }

    pub fn kind(&self) -> u8 {
        match &self.0 {
            StakeCredType::Key(_) => 0,
            StakeCredType::Script(_) => 1,
        }
    }

    fn to_raw_bytes(&self) -> Vec<u8> {
        match &self.0 {
            StakeCredType::Key(hash) => hash.to_bytes(),
            StakeCredType::Script(hash) => hash.to_bytes(),
        }
    }
}



impl cbor_event::se::Serialize for StakeCredential {
    fn serialize<'se, W: Write>(&self, serializer: &'se mut Serializer<W>) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_array(cbor_event::Len::Len(2))?;
        match &self.0 {
            StakeCredType::Key(keyhash) => {
                serializer.write_unsigned_integer(0u64)?;
                serializer.write_bytes(keyhash.to_bytes())
            },
            StakeCredType::Script(scripthash) => {
                serializer.write_unsigned_integer(1u64)?;
                serializer.write_bytes(scripthash.to_bytes())
            },
        }
    }
}

impl Deserialize for StakeCredential {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        (|| -> Result<_, DeserializeError> {
            let len = raw.array()?;
            if let cbor_event::Len::Len(n) = len {
                if n != 2 {
                    return Err(DeserializeFailure::CBOR(cbor_event::Error::WrongLen(2, len, "[id, hash]")).into())
                }
            }
            let cred_type = match raw.unsigned_integer()? {
                0 => StakeCredType::Key(AddrKeyHash::deserialize(raw)?),
                1 => StakeCredType::Script(ScriptHash::deserialize(raw)?),
                n => return Err(DeserializeFailure::FixedValueMismatch{
                    found: Key::Uint(n),
                    // TODO: change codegen to make FixedValueMismatch support Vec<Key> or ranges or something
                    expected: Key::Uint(0),
                }.into()),
            };
            if let cbor_event::Len::Indefinite = len {
                 if raw.special()? != CBORSpecial::Break {
                    return Err(DeserializeFailure::EndingBreakMissing.into());
                }
            }
            Ok(StakeCredential(cred_type))
        })().map_err(|e| e.annotate("StakeCredential"))
    }
}

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd)]
enum AddrType {
    Base(BaseAddress),
    Ptr(PointerAddress),
    Enterprise(EnterpriseAddress),
    Reward(RewardAddress),
}

#[wasm_bindgen]
#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct Address(AddrType);

from_bytes!(Address, data, {
    Self::from_bytes_impl(data.as_ref())
});

// to/from_bytes() are the raw encoding without a wrapping CBOR Bytes tag
// while Serialize and Deserialize traits include that for inclusion with
// other CBOR types
#[wasm_bindgen]
impl Address {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        match &self.0 {
            AddrType::Base(base) => {
                let header: u8 = (base.payment.kind() << 4)
                           | (base.stake.kind() << 5)
                           | (base.network & 0xF);
                buf.push(header);
                buf.extend(base.payment.to_raw_bytes());
                buf.extend(base.stake.to_raw_bytes());
            },
            AddrType::Ptr(ptr) => {
                let header: u8 = 0b0100_0000
                               | (ptr.payment.kind() << 4)
                               | (ptr.network & 0xF);
                buf.push(header);
                buf.extend(ptr.payment.to_raw_bytes());
                buf.extend(variable_nat_encode(ptr.stake.slot));
                buf.extend(variable_nat_encode(ptr.stake.tx_index));
                buf.extend(variable_nat_encode(ptr.stake.cert_index));
            },
            AddrType::Enterprise(enterprise) => {
                let header: u8 = 0b0110_0000
                               | (enterprise.payment.kind() << 4)
                               | (enterprise.network & 0xF);
                buf.push(header);
                buf.extend(enterprise.payment.to_raw_bytes());
            },
            AddrType::Reward(reward) => {
                let header: u8 = 0b1110_0000
                                | (reward.payment.kind() << 4)
                                | (reward.network & 0xF);
                buf.push(header);
                buf.extend(reward.payment.to_raw_bytes());
            },
        }
        println!("to_bytes({:?}) = {:?}", self, buf);
        buf
    }

    fn from_bytes_impl(data: &[u8]) -> Result<Address, DeserializeError> {
        use std::convert::TryInto;
        println!("reading from: {:?}", data);
        // header has 4 bits addr type discrim then 4 bits network discrim.
        // Copied from shelley.cddl:
        //
        // shelley payment addresses:
        // bit 7: 0
        // bit 6: base/other
        // bit 5: pointer/enterprise [for base: stake cred is keyhash/scripthash]
        // bit 4: payment cred is keyhash/scripthash
        // bits 3-0: network id
        //
        // reward addresses:
        // bits 7-5: 111
        // bit 4: credential is keyhash/scripthash
        // bits 3-0: network id
        //
        // byron addresses:
        // bits 7-4: 1000
        (|| -> Result<Self, DeserializeError> {
            let header = data[0];
            let network = header & 0x0F;
            const HASH_LEN: usize = AddrKeyHash::BYTE_COUNT;
            // should be static assert but it's maybe not worth importing a whole external crate for it now
            assert_eq!(ScriptHash::BYTE_COUNT, HASH_LEN);
            let read_addr_cred = |bit: u8, pos: usize| {
                let hash_bytes: [u8; HASH_LEN] = data[pos..pos+HASH_LEN].try_into().unwrap();
                let x = if header & (1 << bit)  == 0 {
                    StakeCredential::from_keyhash(&AddrKeyHash::from(hash_bytes))
                } else {
                    StakeCredential::from_scripthash(&ScriptHash::from(hash_bytes))
                };
                println!("read cred: {:?}", x);
                x
            };
            let addr = match (header & 0xF0) >> 4 {
                // base
                0b0000 | 0b0001 | 0b0010 | 0b0011 => {
                    const BASE_ADDR_SIZE: usize = 1 + HASH_LEN * 2;
                    if data.len() < BASE_ADDR_SIZE {
                        return Err(cbor_event::Error::NotEnough(data.len(), BASE_ADDR_SIZE).into());
                    }
                    if data.len() > BASE_ADDR_SIZE {
                        return Err(cbor_event::Error::TrailingData.into());
                    }
                    AddrType::Base(BaseAddress::new(network, &read_addr_cred(4, 1), &read_addr_cred(5, 1 + HASH_LEN)))
                },
                // pointer
                0b0100 | 0b0101 => {
                    // header + keyhash + 3 natural numbers (min 1 byte each)
                    const PTR_ADDR_MIN_SIZE: usize = 1 + HASH_LEN + 1 + 1 + 1;
                    if data.len() < PTR_ADDR_MIN_SIZE {
                        // possibly more, but depends on how many bytes the natural numbers are for the pointer
                        return Err(cbor_event::Error::NotEnough(data.len(), PTR_ADDR_MIN_SIZE).into());
                    }
                    let mut byte_index = 1;
                    let payment_cred = read_addr_cred(4, 1);
                    byte_index += HASH_LEN;
                    let (slot, slot_bytes) = variable_nat_decode(&data[byte_index..])
                        .ok_or(DeserializeError::new("Address.Pointer.slot", DeserializeFailure::VariableLenNatDecodeFailed))?;
                    byte_index += slot_bytes;
                    let (tx_index, tx_bytes) = variable_nat_decode(&data[byte_index..])
                        .ok_or(DeserializeError::new("Address.Pointer.tx_index", DeserializeFailure::VariableLenNatDecodeFailed))?;
                    byte_index += tx_bytes;
                    let (cert_index, cert_bytes) = variable_nat_decode(&data[byte_index..])
                        .ok_or(DeserializeError::new("Address.Pointer.cert_index", DeserializeFailure::VariableLenNatDecodeFailed))?;
                    byte_index += cert_bytes;
                    if byte_index > data.len() {
                        return Err(cbor_event::Error::TrailingData.into());
                    }
                    AddrType::Ptr(PointerAddress::new(network, &payment_cred, &Pointer::new(slot, tx_index, cert_index)))
                },
                // enterprise
                0b0110 | 0b0111 => {
                    const ENTERPRISE_ADDR_SIZE: usize = 1 + HASH_LEN;
                    if data.len() < ENTERPRISE_ADDR_SIZE {
                        return Err(cbor_event::Error::NotEnough(data.len(), ENTERPRISE_ADDR_SIZE).into());
                    }
                    if data.len() > ENTERPRISE_ADDR_SIZE {
                        return Err(cbor_event::Error::TrailingData.into());
                    }
                    AddrType::Enterprise(EnterpriseAddress::new(network, &read_addr_cred(4, 1)))
                },
                // reward
                0b1110 | 0b1111 => {
                    const REWARD_ADDR_SIZE: usize = 1 + HASH_LEN;
                    if data.len() < REWARD_ADDR_SIZE {
                        return Err(cbor_event::Error::NotEnough(data.len(), REWARD_ADDR_SIZE).into());
                    }
                    if data.len() > REWARD_ADDR_SIZE {
                        return Err(cbor_event::Error::TrailingData.into());
                    }
                    AddrType::Reward(RewardAddress::new(network, &read_addr_cred(4, 1)))
                }
                // byron
                0b1000 => {
                    unimplemented!()
                },
                _ => return Err(DeserializeFailure::BadAddressType(header).into()),
            };
            Ok(Address(addr))
        })().map_err(|e| e.annotate("Address"))
    }

    pub fn to_bech32(&self) -> String {
        bech32::encode("addr", self.to_bytes().to_base32()).unwrap()
    }

    pub fn from_bech32(bech_str: &str) -> Result<Address, JsValue> {
        let (_hrp, u5data) = bech32::decode(bech_str).map_err(|e| JsValue::from_str(&e.to_string()))?;
        let data: Vec<u8> = bech32::FromBase32::from_base32(&u5data).unwrap();
        Self::from_bytes_impl(data.as_ref()).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    pub fn network_id(&self) -> u8 {
        match &self.0 {
            AddrType::Base(a) => a.network,
            AddrType::Enterprise(a) => a.network,
            AddrType::Ptr(a) => a.network,
            AddrType::Reward(a) => a.network,
        }
    }
}

impl cbor_event::se::Serialize for Address {
    fn serialize<'se, W: Write>(&self, serializer: &'se mut Serializer<W>) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_bytes(self.to_bytes())
    }
}

impl Deserialize for Address {
    fn deserialize<R: BufRead>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        Self::from_bytes_impl(raw.bytes()?.as_ref())
    }
}

#[wasm_bindgen]
#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct BaseAddress {
    network: u8,
    payment: StakeCredential,
    stake: StakeCredential,
}

#[wasm_bindgen]
impl BaseAddress {
    pub fn new(network: u8, payment: &StakeCredential, stake: &StakeCredential) -> Self {
        Self {
            network,
            payment: payment.clone(),
            stake: stake.clone(),
        }
    }

    pub fn payment_cred(&self) -> StakeCredential {
        self.payment.clone()
    }

    pub fn stake_cred(&self) -> StakeCredential {
        self.stake.clone()
    }

    pub fn to_address(&self) -> Address {
        Address(AddrType::Base(self.clone()))
    }
}


#[wasm_bindgen]
#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct EnterpriseAddress {
    network: u8,
    payment: StakeCredential,
}

#[wasm_bindgen]
impl EnterpriseAddress {
    pub fn new(network: u8, payment: &StakeCredential) -> Self {
        Self {
            network,
            payment: payment.clone(),
        }
    }

    pub fn payment_cred(&self) -> StakeCredential {
        self.payment.clone()
    }

    pub fn to_address(&self) -> Address {
        Address(AddrType::Enterprise(self.clone()))
    }
}

#[wasm_bindgen]
#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct RewardAddress {
    network: u8,
    payment: StakeCredential,
}

#[wasm_bindgen]
impl RewardAddress {
    pub fn new(network: u8, payment: &StakeCredential) -> Self {
        Self {
            network,
            payment: payment.clone(),
        }
    }

    pub fn payment_cred(&self) -> StakeCredential {
        self.payment.clone()
    }

    pub fn to_address(&self) -> Address {
        Address(AddrType::Reward(self.clone()))
    }
}

// needed since we treat RewardAccount like RewardAddress
impl cbor_event::se::Serialize for RewardAddress {
    fn serialize<'se, W: Write>(&self, serializer: &'se mut Serializer<W>) -> cbor_event::Result<&'se mut Serializer<W>> {
        self.to_address().serialize(serializer)
    }
}

impl Deserialize for RewardAddress {
    fn deserialize<R: BufRead>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        (|| -> Result<Self, DeserializeError> {
            let bytes = raw.bytes()?;
            match Address::from_bytes_impl(bytes.as_ref())?.0 {
                AddrType::Reward(ra) => Ok(ra),
                other_address => Err(DeserializeFailure::BadAddressType(bytes[0]).into()),
            }
        })().map_err(|e| e.annotate("RewardAddress"))
    }
}

#[wasm_bindgen]
#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct Pointer {
    slot: u64,
    tx_index: u64,
    cert_index: u64,
}

#[wasm_bindgen]
impl Pointer {
    pub fn new(slot: u64, tx_index: u64, cert_index: u64) -> Self {
        Self {
            slot,
            tx_index,
            cert_index,
        }
    }
}

#[wasm_bindgen]
#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct PointerAddress {
    network: u8,
    payment: StakeCredential,
    stake: Pointer,
}

#[wasm_bindgen]
impl PointerAddress {
    pub fn new(network: u8, payment: &StakeCredential, stake: &Pointer) -> Self {
        Self {
            network,
            payment: payment.clone(),
            stake: stake.clone(),
        }
    }

    pub fn payment_cred(&self) -> StakeCredential {
        self.payment.clone()
    }

    pub fn stake_ponter(&self) -> Pointer {
        self.stake.clone()
    }

    pub fn to_address(&self) -> Address {
        Address(AddrType::Ptr(self.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crypto::*;

    #[test]
    fn variable_nat_encoding() {
        let cases = [
            0u64,
            127u64,
            128u64,
            255u64,
            256275757658493284u64
        ];
        for case in cases.iter() {
            let encoded = variable_nat_encode(*case);
            let decoded = variable_nat_decode(&encoded).unwrap().0;
            assert_eq!(*case, decoded);
        }
    }

    #[test]
    fn base_serialize_consistency() {
        let base = BaseAddress::new(
            5,
            &StakeCredential::from_keyhash(&AddrKeyHash::from([23; AddrKeyHash::BYTE_COUNT])),
            &StakeCredential::from_scripthash(&ScriptHash::from([42; ScriptHash::BYTE_COUNT])));
        let addr = base.to_address();
        let addr2 = Address::from_bytes(addr.to_bytes()).unwrap();
        assert_eq!(addr.to_bytes(), addr2.to_bytes());
    }

    #[test]
    fn ptr_serialize_consistency() {
        let ptr = PointerAddress::new(
            25,
            &StakeCredential::from_keyhash(&AddrKeyHash::from([23; AddrKeyHash::BYTE_COUNT])),
            &Pointer::new(2354556573, 127, 0));
        let addr = ptr.to_address();
        let addr2 = Address::from_bytes(addr.to_bytes()).unwrap();
        assert_eq!(addr.to_bytes(), addr2.to_bytes());
    }

    #[test]
    fn enterprise_serialize_consistency() {
        let enterprise = EnterpriseAddress::new(
            64,
            &StakeCredential::from_keyhash(&AddrKeyHash::from([23; AddrKeyHash::BYTE_COUNT])));
        let addr = enterprise.to_address();
        let addr2 = Address::from_bytes(addr.to_bytes()).unwrap();
        assert_eq!(addr.to_bytes(), addr2.to_bytes());
    }

    #[test]
    fn reward_serialize_consistency() {
        let reward = RewardAddress::new(
            9,
            &StakeCredential::from_scripthash(&ScriptHash::from([127; AddrKeyHash::BYTE_COUNT])));
        let addr = reward.to_address();
        let addr2 = Address::from_bytes(addr.to_bytes()).unwrap();
        assert_eq!(addr.to_bytes(), addr2.to_bytes());
    }

    fn root_key_12() -> Bip32PrivateKey {
        let entropy = [0xdf, 0x9e, 0xd2, 0x5e, 0xd1, 0x46, 0xbf, 0x43, 0x33, 0x6a, 0x5d, 0x7c, 0xf7, 0x39, 0x59, 0x94];
        Bip32PrivateKey::from_bip39_entropy(&entropy, &[])
    }

    fn root_key_15() -> Bip32PrivateKey {
        let entropy = [0x0c, 0xcb, 0x74, 0xf3, 0x6b, 0x7d, 0xa1, 0x64, 0x9a, 0x81, 0x44, 0x67, 0x55, 0x22, 0xd4, 0xd8, 0x09, 0x7c, 0x64, 0x12];
        Bip32PrivateKey::from_bip39_entropy(&entropy, &[])
    }

    fn root_key_24() -> Bip32PrivateKey {
        let entropy = [0x4e, 0x82, 0x8f, 0x9a, 0x67, 0xdd, 0xcf, 0xf0, 0xe6, 0x39, 0x1a, 0xd4, 0xf2, 0x6d, 0xdb, 0x75, 0x79, 0xf5, 0x9b, 0xa1, 0x4b, 0x6d, 0xd4, 0xba, 0xf6, 0x3d, 0xcf, 0xdb, 0x9d, 0x24, 0x20, 0xda];
        Bip32PrivateKey::from_bip39_entropy(&entropy, &[])
    }

    fn harden(index: u32) -> u32 {
        index | 0x80_00_00_00
    }

    #[test]
    fn bip32_12_base() {
        let spend = root_key_12()
            .derive(harden(1852))
            .derive(harden(1815))
            .derive(harden(0))
            .derive(0)
            .derive(0)
            .to_public();
        let stake = root_key_12()
            .derive(harden(1852))
            .derive(harden(1815))
            .derive(harden(0))
            .derive(2)
            .derive(0)
            .to_public();
        let spend_cred = StakeCredential::from_keyhash(&spend.hash());
        let stake_cred = StakeCredential::from_keyhash(&stake.hash());
        let addr_net_0 = BaseAddress::new(0, &spend_cred, &stake_cred).to_address();
        assert_eq!(addr_net_0.to_bech32(), "addr1qz2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzer3jcu5d8ps7zex2k2xt3uqxgjqnnj83ws8lhrn648jjxtwqcyl47r");
        let addr_net_3 = BaseAddress::new(3, &spend_cred, &stake_cred).to_address();
        assert_eq!(addr_net_3.to_bech32(), "addr1qw2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzer3jcu5d8ps7zex2k2xt3uqxgjqnnj83ws8lhrn648jjxtwqzhyupd");
    }

    #[test]
    fn bip32_12_enterprise() {
        let spend = root_key_12()
            .derive(harden(1852))
            .derive(harden(1815))
            .derive(harden(0))
            .derive(0)
            .derive(0)
            .to_public();
        let spend_cred = StakeCredential::from_keyhash(&spend.hash());
        let addr_net_0 = EnterpriseAddress::new(0, &spend_cred).to_address();
        assert_eq!(addr_net_0.to_bech32(), "addr1vz2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzers6g8jlq");
        let addr_net_3 = EnterpriseAddress::new(3, &spend_cred).to_address();
        assert_eq!(addr_net_3.to_bech32(), "addr1vw2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzers6h7glf");
    }

    #[test]
    fn bip32_12_pointer() {
        let spend = root_key_12()
            .derive(harden(1852))
            .derive(harden(1815))
            .derive(harden(0))
            .derive(0)
            .derive(0)
            .to_public();
        let spend_cred = StakeCredential::from_keyhash(&spend.hash());
        let addr_net_0 = PointerAddress::new(0, &spend_cred, &Pointer::new(1, 2, 3)).to_address();
        assert_eq!(addr_net_0.to_bech32(), "addr1gz2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzerspqgpslhplej");
        let addr_net_3 = PointerAddress::new(3, &spend_cred, &Pointer::new(24157, 177, 42)).to_address();
        assert_eq!(addr_net_3.to_bech32(), "addr1gw2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzer5ph3wczvf2x4v58t");
    }

    #[test]
    fn bip32_15_base() {
        let spend = root_key_15()
            .derive(harden(1852))
            .derive(harden(1815))
            .derive(harden(0))
            .derive(0)
            .derive(0)
            .to_public();
        let stake = root_key_15()
            .derive(harden(1852))
            .derive(harden(1815))
            .derive(harden(0))
            .derive(2)
            .derive(0)
            .to_public();
        let spend_cred = StakeCredential::from_keyhash(&spend.hash());
        let stake_cred = StakeCredential::from_keyhash(&stake.hash());
        let addr_net_0 = BaseAddress::new(0, &spend_cred, &stake_cred).to_address();
        assert_eq!(addr_net_0.to_bech32(), "addr1qpu5vlrf4xkxv2qpwngf6cjhtw542ayty80v8dyr49rf5ewvxwdrt70qlcpeeagscasafhffqsxy36t90ldv06wqrk2qwmnp2v");
        let addr_net_3 = BaseAddress::new(3, &spend_cred, &stake_cred).to_address();
        assert_eq!(addr_net_3.to_bech32(), "addr1qdu5vlrf4xkxv2qpwngf6cjhtw542ayty80v8dyr49rf5ewvxwdrt70qlcpeeagscasafhffqsxy36t90ldv06wqrk2q5ggg4z");
    }

    #[test]
    fn bip32_15_enterprise() {
        let spend = root_key_15()
            .derive(harden(1852))
            .derive(harden(1815))
            .derive(harden(0))
            .derive(0)
            .derive(0)
            .to_public();
        let spend_cred = StakeCredential::from_keyhash(&spend.hash());
        let addr_net_0 = EnterpriseAddress::new(0, &spend_cred).to_address();
        assert_eq!(addr_net_0.to_bech32(), "addr1vpu5vlrf4xkxv2qpwngf6cjhtw542ayty80v8dyr49rf5eg0yu80w");
        let addr_net_3 = EnterpriseAddress::new(3, &spend_cred).to_address();
        assert_eq!(addr_net_3.to_bech32(), "addr1vdu5vlrf4xkxv2qpwngf6cjhtw542ayty80v8dyr49rf5eg0m9a08");
    }

    #[test]
    fn bip32_15_pointer() {
        let spend = root_key_15()
            .derive(harden(1852))
            .derive(harden(1815))
            .derive(harden(0))
            .derive(0)
            .derive(0)
            .to_public();
        let spend_cred = StakeCredential::from_keyhash(&spend.hash());
        let addr_net_0 = PointerAddress::new(0, &spend_cred, &Pointer::new(1, 2, 3)).to_address();
        assert_eq!(addr_net_0.to_bech32(), "addr1gpu5vlrf4xkxv2qpwngf6cjhtw542ayty80v8dyr49rf5egpqgpsjej5ck");
        let addr_net_3 = PointerAddress::new(3, &spend_cred, &Pointer::new(24157, 177, 42)).to_address();
        assert_eq!(addr_net_3.to_bech32(), "addr1gdu5vlrf4xkxv2qpwngf6cjhtw542ayty80v8dyr49rf5evph3wczvf27l8yfx");
    }

    #[test]
    fn bip32_24_base() {
        let spend = root_key_24()
            .derive(harden(1852))
            .derive(harden(1815))
            .derive(harden(0))
            .derive(0)
            .derive(0)
            .to_public();
        let stake = root_key_24()
            .derive(harden(1852))
            .derive(harden(1815))
            .derive(harden(0))
            .derive(2)
            .derive(0)
            .to_public();
        let spend_cred = StakeCredential::from_keyhash(&spend.hash());
        let stake_cred = StakeCredential::from_keyhash(&stake.hash());
        let addr_net_0 = BaseAddress::new(0, &spend_cred, &stake_cred).to_address();
        assert_eq!(addr_net_0.to_bech32(), "addr1qqy6nhfyks7wdu3dudslys37v252w2nwhv0fw2nfawemmn8k8ttq8f3gag0h89aepvx3xf69g0l9pf80tqv7cve0l33su9wxrs");
        let addr_net_3 = BaseAddress::new(3, &spend_cred, &stake_cred).to_address();
        assert_eq!(addr_net_3.to_bech32(), "addr1qvy6nhfyks7wdu3dudslys37v252w2nwhv0fw2nfawemmn8k8ttq8f3gag0h89aepvx3xf69g0l9pf80tqv7cve0l33sxk40u7");
    }

    #[test]
    fn bip32_24_enterprise() {
        let spend = root_key_24()
            .derive(harden(1852))
            .derive(harden(1815))
            .derive(harden(0))
            .derive(0)
            .derive(0)
            .to_public();
        let spend_cred = StakeCredential::from_keyhash(&spend.hash());
        let addr_net_0 = EnterpriseAddress::new(0, &spend_cred).to_address();
        assert_eq!(addr_net_0.to_bech32(), "addr1vqy6nhfyks7wdu3dudslys37v252w2nwhv0fw2nfawemmnqsg0y49");
        let addr_net_3 = EnterpriseAddress::new(3, &spend_cred).to_address();
        assert_eq!(addr_net_3.to_bech32(), "addr1vvy6nhfyks7wdu3dudslys37v252w2nwhv0fw2nfawemmnqshk74v");
    }

    #[test]
    fn bip32_24_pointer() {
        let spend = root_key_24()
            .derive(harden(1852))
            .derive(harden(1815))
            .derive(harden(0))
            .derive(0)
            .derive(0)
            .to_public();
        let spend_cred = StakeCredential::from_keyhash(&spend.hash());
        let addr_net_0 = PointerAddress::new(0, &spend_cred, &Pointer::new(1, 2, 3)).to_address();
        assert_eq!(addr_net_0.to_bech32(), "addr1gqy6nhfyks7wdu3dudslys37v252w2nwhv0fw2nfawemmnqpqgpst4xf0c");
        let addr_net_3 = PointerAddress::new(3, &spend_cred, &Pointer::new(24157, 177, 42)).to_address();
        assert_eq!(addr_net_3.to_bech32(), "addr1gvy6nhfyks7wdu3dudslys37v252w2nwhv0fw2nfawemmnyph3wczvf29j6huk");
    }
}