use crate::parser::types::ParamInfo;
use crate::util::hash::sha256;

pub fn compute_signature_hash(
    name: &str,
    params: &[ParamInfo],
    return_type: Option<&str>,
) -> String {
    let normalized_name = name.to_lowercase().trim().to_string();

    let normalized_params: String = params
        .iter()
        .map(|p| {
            let t = p.type_.as_deref().unwrap_or("any");
            t.split_whitespace().collect::<String>().to_lowercase()
        })
        .collect::<Vec<_>>()
        .join(",");

    let normalized_return = {
        let t = return_type.unwrap_or("void");
        t.split_whitespace().collect::<String>().to_lowercase()
    };

    let input = format!("{normalized_name}({normalized_params}):{normalized_return}");
    sha256(&input)[..8].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn produces_8_char_hex() {
        let hash = compute_signature_hash("myFunc", &[], None);
        assert_eq!(hash.len(), 8);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn deterministic() {
        let params = vec![ParamInfo {
            name: "x".into(),
            type_: Some("number".into()),
        }];
        let h1 = compute_signature_hash("foo", &params, Some("string"));
        let h2 = compute_signature_hash("foo", &params, Some("string"));
        assert_eq!(h1, h2);
    }

    #[test]
    fn case_insensitive_name() {
        let h1 = compute_signature_hash("MyFunc", &[], None);
        let h2 = compute_signature_hash("myfunc", &[], None);
        assert_eq!(h1, h2);
    }

    #[test]
    fn whitespace_agnostic_types() {
        let p1 = vec![ParamInfo {
            name: "x".into(),
            type_: Some("Map<string, number>".into()),
        }];
        let p2 = vec![ParamInfo {
            name: "x".into(),
            type_: Some("Map<string,number>".into()),
        }];
        let h1 = compute_signature_hash("f", &p1, None);
        let h2 = compute_signature_hash("f", &p2, None);
        assert_eq!(h1, h2);
    }

    #[test]
    fn different_types_differ() {
        let p1 = vec![ParamInfo {
            name: "x".into(),
            type_: Some("number".into()),
        }];
        let p2 = vec![ParamInfo {
            name: "x".into(),
            type_: Some("string".into()),
        }];
        let h1 = compute_signature_hash("f", &p1, None);
        let h2 = compute_signature_hash("f", &p2, None);
        assert_ne!(h1, h2);
    }

    #[test]
    fn null_defaults() {
        let params = vec![ParamInfo {
            name: "x".into(),
            type_: None,
        }];
        // null type defaults to "any", null return defaults to "void"
        let hash = compute_signature_hash("f", &params, None);
        assert_eq!(hash.len(), 8);
    }
}
