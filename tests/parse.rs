use samp_mirror::api::parse_content_type;

#[test]
fn test_parse_hex_prefix() {
    assert_eq!(parse_content_type("0x10"), Some(0x10));
    assert_eq!(parse_content_type("0xff"), Some(0xff));
    assert_eq!(parse_content_type("0x00"), Some(0x00));
}

#[test]
fn test_parse_decimal() {
    assert_eq!(parse_content_type("16"), Some(16));
    assert_eq!(parse_content_type("255"), Some(255));
    assert_eq!(parse_content_type("0"), Some(0));
}

#[test]
fn test_parse_invalid() {
    assert_eq!(parse_content_type("xyz"), None);
    assert_eq!(parse_content_type("0xgg"), None);
    assert_eq!(parse_content_type(""), None);
    assert_eq!(parse_content_type("256"), None);
}
