use bitcoin::hashes::{Hash, HashEngine};

bitcoin::hashes::hash_newtype! {
    /// A script hash used by Electrum to identify wallet outputs.
    ///
    /// This is the value passed to methods like `blockchain.scripthash.get_balance`,
    /// `get_history`, `listunspent`, and subscription requests.
    ///
    /// It wraps a SHA256 hash of the script **in reversed byte order**, as required by the Electrum
    /// protocol. This is different from how script hashes are typically represented in Bitcoin.
    ///
    /// Use [`ElectrumScriptHash::from_script`] to compute this value from a Bitcoin [`Script`].
    ///
    /// [`Script`]: bitcoin::Script
    #[hash_newtype(backward)]
    pub struct ElectrumScriptHash(bitcoin::hashes::sha256::Hash);

    /// Represents the Electrum server's status hash for a specific script.
    ///
    /// This is the `status` field returned by methods like
    /// `blockchain.scripthash.subscribe` and `blockchain.scripthash.get_history`.
    ///
    /// The hash summarizes the confirmed and unconfirmed state of a script. If the status changes,
    /// clients should re-query the history and unspent outputs for the script.
    ///
    /// Internally, it wraps a `sha256` hash used by Electrum for detecting state changes.
    pub struct ElectrumScriptStatus(bitcoin::hashes::sha256::Hash);
}

impl ElectrumScriptHash {
    /// Computes a new [`ElectrumScriptHash`] from the given script.
    ///
    /// This performs a SHA256 hash of the script and then reverses the byte order,
    /// as required by the Electrum protocol.
    ///
    /// This is the standard way to obtain a script hash for use with Electrum server methods like
    /// `blockchain.scripthash.get_balance` or `blockchain.scripthash.subscribe`.
    pub fn new(script: impl AsRef<bitcoin::Script>) -> Self {
        use bitcoin::hashes::Hash;

        ElectrumScriptHash(bitcoin::hashes::sha256::Hash::hash(
            script.as_ref().as_bytes(),
        ))
    }
}

impl From<bitcoin::ScriptBuf> for ElectrumScriptHash {
    fn from(value: bitcoin::ScriptBuf) -> Self {
        Self::new(value)
    }
}

impl From<&bitcoin::Script> for ElectrumScriptHash {
    fn from(value: &bitcoin::Script) -> Self {
        Self::new(value)
    }
}

impl ElectrumScriptStatus {
    pub fn from_history(history: &[crate::response::Tx]) -> Option<Self> {
        if history.is_empty() {
            return None;
        }

        let mut engine = bitcoin::hashes::sha256::Hash::engine();
        for tx in history {
            let s = format!("{}:{}:", tx.txid(), tx.electrum_height());
            engine.input(s.as_bytes());
        }
        Some(Self::from_engine(engine))
    }
}
