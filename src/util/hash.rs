use sha2::{Digest, Sha256};

pub fn sha256(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn content_hash(source: &str) -> String {
    sha256(source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_deterministic() {
        let hash1 = sha256("hello world");
        let hash2 = sha256("hello world");
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 64);
    }

    #[test]
    fn sha256_different_inputs() {
        assert_ne!(sha256("hello"), sha256("world"));
    }

    #[test]
    fn content_hash_is_sha256() {
        assert_eq!(content_hash("test"), sha256("test"));
    }
}
