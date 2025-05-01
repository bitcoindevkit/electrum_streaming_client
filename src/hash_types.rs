bitcoin::hashes::hash_newtype! {
    /// Hash of the script pubkey
    #[hash_newtype(backward)]
    pub struct ElectrumScriptHash(bitcoin::hashes::sha256::Hash);

    pub struct ElectrumScriptStatus(bitcoin::hashes::sha256::Hash);
}

impl ElectrumScriptHash {
    pub fn new(script: &bitcoin::Script) -> Self {
        use bitcoin::hashes::Hash;

        ElectrumScriptHash(bitcoin::hashes::sha256::Hash::hash(script.as_bytes()))
    }
}
