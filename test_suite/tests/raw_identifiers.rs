// Fields named with raw identifiers (`r#type`) go through every generated
// item: construction, the builder, rewriting and serialization. Building the
// rewrite locals from such a name used to panic the macro.

use hirpdag::*;

#[hirpdag_module]
mod datamodel {
    #[hirpdag(root)]
    pub struct Token {
        pub r#type: String,
        pub r#match: Option<Token>,
    }
}

use datamodel::*;

struct Rename;

impl HirpdagRewriter for Rename {
    fn rewrite_Token<D: HirpdagRewriteDriver>(&self, x: &Token, driver: &D) -> Token {
        if x.r#match.is_none() {
            return Token::new(format!("renamed_{}", x.r#type), None);
        }
        x.default_rewrite(driver)
    }
}

#[test]
fn raw_identifier_fields_work_end_to_end() {
    let leaf = Token::new("raw_leaf".to_string(), None);
    let parent = Token::builder()
        .r#type("raw_parent".to_string())
        .r#match(Some(leaf.clone()))
        .build();
    assert_eq!(parent, Token::new("raw_parent".to_string(), Some(leaf)));

    let rewritten = HirpdagRewriteMemoized::new(Rename).rewrite(&parent);
    assert_eq!(
        rewritten.r#match.as_ref().map(|m| m.r#type.as_str()),
        Some("renamed_raw_leaf")
    );

    let roots = HirpdagArchiveRoots {
        roots_Token: vec![parent.clone()],
    };
    let json = hirpdag_serialize_json(&roots).unwrap();
    assert!(json.contains("\"type\""), "serde strips the r#: {json}");
    assert_eq!(hirpdag_deserialize_json(&json).unwrap(), roots);
    let bytes = hirpdag_serialize(&roots).unwrap();
    assert_eq!(hirpdag_deserialize(&bytes).unwrap(), roots);
}
