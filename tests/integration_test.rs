use assert_cmd::Command;
use predicates::str::contains;

fn cq() -> Command {
    Command::cargo_bin("cq").unwrap()
}

// --- Identity round-trip golden tests ---

#[test]
fn identity_record() {
    cq()
        .write_stdin("(record { foo = 1 : nat })")
        .assert()
        .success()
        .stdout("(record { foo = 1 : nat })\n");
}

#[test]
fn identity_variant() {
    cq()
        .write_stdin("(variant { Ok = \"hello\" })")
        .assert()
        .success()
        .stdout("(variant { Ok = \"hello\" })\n");
}

#[test]
fn identity_opt_some() {
    cq()
        .write_stdin("(opt (42 : nat32))")
        .assert()
        .success()
        .stdout("(opt (42 : nat32))\n");
}

#[test]
fn identity_vec() {
    cq()
        .write_stdin("(vec { 1 : nat; 2 : nat; 3 : nat })")
        .assert()
        .success()
        .stdout("(vec { 1 : nat; 2 : nat; 3 : nat })\n");
}

#[test]
fn identity_bool() {
    cq()
        .write_stdin("(true)")
        .assert()
        .success()
        .stdout("(true)\n");
}

#[test]
fn identity_int() {
    cq()
        .write_stdin("(42 : int)")
        .assert()
        .success()
        .stdout("(42 : int)\n");
}

#[test]
fn identity_text() {
    cq()
        .write_stdin("(\"hello world\")")
        .assert()
        .success()
        .stdout("(\"hello world\")\n");
}

#[test]
fn identity_principal() {
    cq()
        .write_stdin("(principal \"aaaaa-aa\")")
        .assert()
        .success()
        .stdout("(principal \"aaaaa-aa\")\n");
}

#[test]
fn identity_blob() {
    cq()
        .write_stdin("(blob \"hello\")")
        .assert()
        .success()
        .stdout("(blob \"hello\")\n");
}

#[test]
fn identity_null() {
    cq()
        .write_stdin("(null)")
        .assert()
        .success()
        .stdout("(null : null)\n");
}

// --- Error handling ---

#[test]
fn parse_failure_exits_nonzero() {
    cq()
        .write_stdin("not valid candid")
        .assert()
        .failure()
        .stderr(contains("failed to parse Candid text"));
}

#[test]
fn empty_input_produces_no_output() {
    cq()
        .write_stdin("")
        .assert()
        .success()
        .stdout("");
}

#[test]
fn invalid_hex_odd_length_errors() {
    cq()
        .arg("--input-format")
        .arg("hex")
        .write_stdin("abc")
        .assert()
        .failure()
        .stderr(contains("invalid hex"));
}

#[test]
fn invalid_hex_non_hex_chars_errors() {
    cq()
        .arg("--input-format")
        .arg("hex")
        .write_stdin("zz")
        .assert()
        .failure()
        .stderr(contains("invalid hex"));
}

#[test]
fn truncated_binary_errors() {
    cq()
        .arg("--input-format")
        .arg("bin")
        .write_stdin("DIDL")
        .assert()
        .failure()
        .stderr(contains("cq:"));
}

// --- Text format alias ---

#[test]
fn output_text_alias_for_candid() {
    cq()
        .arg("--output")
        .arg("text")
        .write_stdin("(42 : nat)")
        .assert()
        .success()
        .stdout("(42 : nat)\n");
}

#[test]
fn input_format_candid_alias_for_text() {
    cq()
        .arg("--input-format")
        .arg("candid")
        .write_stdin("(42 : nat)")
        .assert()
        .success()
        .stdout("(42 : nat)\n");
}

// --- Multi-value streaming ---

#[test]
fn multi_value_text_input() {
    cq()
        .write_stdin("(42 : nat)(99 : nat)")
        .assert()
        .success()
        .stdout("(42 : nat)\n(99 : nat)\n");
}

#[test]
fn multi_value_text_input_newline_separated() {
    cq()
        .write_stdin("(42 : nat)\n(99 : nat)\n")
        .assert()
        .success()
        .stdout("(42 : nat)\n(99 : nat)\n");
}

#[test]
fn multi_value_hex_input() {
    // Two frames: (42 : nat) and (99 : nat)
    cq()
        .arg("--input-format")
        .arg("hex")
        .write_stdin("4449444c00017d2a\n4449444c00017d63\n")
        .assert()
        .success()
        .stdout("(42 : nat)\n(99 : nat)\n");
}

#[test]
fn multi_value_bin_parse_and_advance() {
    // Two concatenated DIDL frames for (42 : nat) and (99 : nat)
    let frame1 = hex::decode("4449444c00017d2a").unwrap();
    let frame2 = hex::decode("4449444c00017d63").unwrap();
    let mut both = frame1;
    both.extend_from_slice(&frame2);
    cq()
        .arg("--input-format")
        .arg("bin")
        .write_stdin(both.as_slice())
        .assert()
        .success()
        .stdout("(42 : nat)\n(99 : nat)\n");
}

// --- 3x3 round-trip matrix ---
// Binary encoding replaces field names with hashes; use (42 : nat) to avoid
// that complication and get a clean round-trip for all 9 pairs.

fn text_for_nat42() -> &'static str {
    "(42 : nat)"
}

fn hex_for_nat42() -> &'static str {
    "4449444c00017d2a"
}

fn bin_for_nat42() -> Vec<u8> {
    hex::decode("4449444c00017d2a").unwrap()
}

#[test]
fn roundtrip_text_to_text() {
    cq()
        .write_stdin(text_for_nat42())
        .assert()
        .success()
        .stdout(format!("{}\n", text_for_nat42()));
}

#[test]
fn roundtrip_text_to_hex() {
    cq()
        .arg("--output")
        .arg("hex")
        .write_stdin(text_for_nat42())
        .assert()
        .success()
        .stdout(format!("{}\n", hex_for_nat42()));
}

#[test]
fn roundtrip_text_to_bin() {
    cq()
        .arg("--output")
        .arg("bin")
        .write_stdin(text_for_nat42())
        .assert()
        .success()
        .stdout(bin_for_nat42());
}

#[test]
fn roundtrip_hex_to_text() {
    cq()
        .arg("--input-format")
        .arg("hex")
        .write_stdin(hex_for_nat42())
        .assert()
        .success()
        .stdout(format!("{}\n", text_for_nat42()));
}

#[test]
fn roundtrip_hex_to_hex() {
    cq()
        .arg("--input-format")
        .arg("hex")
        .arg("--output")
        .arg("hex")
        .write_stdin(hex_for_nat42())
        .assert()
        .success()
        .stdout(format!("{}\n", hex_for_nat42()));
}

#[test]
fn roundtrip_hex_to_bin() {
    cq()
        .arg("--input-format")
        .arg("hex")
        .arg("--output")
        .arg("bin")
        .write_stdin(hex_for_nat42())
        .assert()
        .success()
        .stdout(bin_for_nat42());
}

#[test]
fn roundtrip_bin_to_text() {
    cq()
        .arg("--input-format")
        .arg("bin")
        .write_stdin(bin_for_nat42().as_slice())
        .assert()
        .success()
        .stdout(format!("{}\n", text_for_nat42()));
}

#[test]
fn roundtrip_bin_to_hex() {
    cq()
        .arg("--input-format")
        .arg("bin")
        .arg("--output")
        .arg("hex")
        .write_stdin(bin_for_nat42().as_slice())
        .assert()
        .success()
        .stdout(format!("{}\n", hex_for_nat42()));
}

#[test]
fn roundtrip_bin_to_bin() {
    cq()
        .arg("--input-format")
        .arg("bin")
        .arg("--output")
        .arg("bin")
        .write_stdin(bin_for_nat42().as_slice())
        .assert()
        .success()
        .stdout(bin_for_nat42());
}

// --- Slice 3: field access and pipe ---

#[test]
fn field_access_simple() {
    cq()
        .args([".foo"])
        .write_stdin("(record { foo = 42 : nat })")
        .assert()
        .success()
        .stdout("(42 : nat)\n");
}

#[test]
fn field_access_chained() {
    cq()
        .args([".foo.bar"])
        .write_stdin("(record { foo = record { bar = 7 : nat } })")
        .assert()
        .success()
        .stdout("(7 : nat)\n");
}

#[test]
fn field_access_three_levels() {
    cq()
        .args([".a.b.c"])
        .write_stdin("(record { a = record { b = record { c = 99 : nat } } })")
        .assert()
        .success()
        .stdout("(99 : nat)\n");
}

#[test]
fn pipe_field_access() {
    cq()
        .args([".foo | .bar"])
        .write_stdin("(record { foo = record { bar = 3 : nat } })")
        .assert()
        .success()
        .stdout("(3 : nat)\n");
}

#[test]
fn pipe_identity_passthrough() {
    cq()
        .args([". | .foo"])
        .write_stdin("(record { foo = 5 : nat })")
        .assert()
        .success()
        .stdout("(5 : nat)\n");
}

#[test]
fn identity_explicit_dot() {
    cq()
        .args(["."])
        .write_stdin("(record { foo = 1 : nat })")
        .assert()
        .success()
        .stdout("(record { foo = 1 : nat })\n");
}

#[test]
fn field_access_unknown_field_errors() {
    cq()
        .args([".missing"])
        .write_stdin("(record { foo = 1 : nat })")
        .assert()
        .failure()
        .stderr(predicates::str::contains("unknown field 'missing'"))
        .stderr(predicates::str::contains("foo"));
}

#[test]
fn field_access_non_record_errors() {
    cq()
        .args([".foo"])
        .write_stdin("(42 : nat)")
        .assert()
        .failure()
        .stderr(predicates::str::contains("field access '.foo' requires a record"));
}

#[test]
fn field_access_text_field() {
    cq()
        .args([".name"])
        .write_stdin("(record { name = \"alice\" })")
        .assert()
        .success()
        .stdout("(\"alice\")\n");
}

#[test]
fn field_access_round_trip_pipe_cq() {
    // cq '.foo' | cq '.' produces same bytes as cq '.foo' alone
    let direct = cq()
        .args([".foo"])
        .write_stdin("(record { foo = 1 : nat })")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let piped = cq()
        .args(["."])
        .write_stdin(direct.as_slice())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(direct, piped);
}

// --- Slice 4: vec ops ---

#[test]
fn vec_index_first() {
    cq()
        .args([".[0]"])
        .write_stdin("(vec { 1 : nat; 2 : nat; 3 : nat })")
        .assert()
        .success()
        .stdout("(1 : nat)\n");
}

#[test]
fn vec_index_last() {
    cq()
        .args([".[2]"])
        .write_stdin("(vec { 1 : nat; 2 : nat; 3 : nat })")
        .assert()
        .success()
        .stdout("(3 : nat)\n");
}

#[test]
fn vec_index_out_of_bounds_errors() {
    cq()
        .args([".[5]"])
        .write_stdin("(vec { 1 : nat; 2 : nat; 3 : nat })")
        .assert()
        .failure()
        .stderr(contains("out of bounds"));
}

#[test]
fn vec_slice_returns_subvec() {
    cq()
        .args([".[1:3]"])
        .write_stdin("(vec { 1 : nat; 2 : nat; 3 : nat; 4 : nat })")
        .assert()
        .success()
        .stdout("(vec { 2 : nat; 3 : nat })\n");
}

#[test]
fn vec_iter_produces_stream() {
    cq()
        .args([".[]"])
        .write_stdin("(vec { 1 : nat; 2 : nat; 3 : nat })")
        .assert()
        .success()
        .stdout("(1 : nat)\n(2 : nat)\n(3 : nat)\n");
}

#[test]
fn vec_iter_empty_vec_no_output() {
    cq()
        .args([".[]"])
        .write_stdin("(vec {})")
        .assert()
        .success()
        .stdout("");
}

#[test]
fn vec_iter_wrong_type_errors() {
    cq()
        .args([".[]"])
        .write_stdin("(42 : nat)")
        .assert()
        .failure()
        .stderr(contains("iterator '.[]' requires a vec"));
}

#[test]
fn vec_index_wrong_type_errors() {
    cq()
        .args([".[0]"])
        .write_stdin("(42 : nat)")
        .assert()
        .failure()
        .stderr(contains("index access '.[0]' requires a vec"));
}

#[test]
fn vec_iter_round_trip() {
    // cq '.[]' | cq '.' produces the same elements
    let exploded = cq()
        .args([".[]"])
        .write_stdin("(vec { 1 : nat; 2 : nat; 3 : nat })")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    cq()
        .args(["."])
        .write_stdin(exploded.as_slice())
        .assert()
        .success()
        .stdout("(1 : nat)\n(2 : nat)\n(3 : nat)\n");
}

#[test]
fn vec_chained_index_then_field() {
    cq()
        .args([".[0].name"])
        .write_stdin("(vec { record { name = \"alice\" } })")
        .assert()
        .success()
        .stdout("(\"alice\")\n");
}

#[test]
fn vec_field_then_index() {
    cq()
        .args([".items.[1]"])
        .write_stdin("(record { items = vec { 10 : nat; 20 : nat; 30 : nat } })")
        .assert()
        .success()
        .stdout("(20 : nat)\n");
}

// --- Slice 5: variant arm access ---

#[test]
fn variant_tag_access_matching_arm() {
    cq()
        .args([".Ok"])
        .write_stdin("(variant { Ok = \"hello\" })")
        .assert()
        .success()
        .stdout("(\"hello\")\n");
}

#[test]
fn variant_tag_access_wrong_arm_errors() {
    cq()
        .args([".Err"])
        .write_stdin("(variant { Ok = \"hello\" })")
        .assert()
        .failure()
        .stderr(contains("tag mismatch"))
        .stderr(contains("Ok"))
        .stderr(contains("Err"));
}

#[test]
fn variant_null_payload_returns_null() {
    cq()
        .args([".Pending"])
        .write_stdin("(variant { Pending })")
        .assert()
        .success()
        .stdout("(null : null)\n");
}

#[test]
fn variant_tag_optional_skip_on_mismatch() {
    cq()
        .args([".Err?"])
        .write_stdin("(variant { Ok = \"hello\" })")
        .assert()
        .success()
        .stdout("");
}

#[test]
fn variant_tag_optional_returns_payload_on_match() {
    cq()
        .args([".Ok?"])
        .write_stdin("(variant { Ok = \"hello\" })")
        .assert()
        .success()
        .stdout("(\"hello\")\n");
}

#[test]
fn variant_field_access_regression() {
    // .Tag on a record still works like slice 3 field access
    cq()
        .args([".foo"])
        .write_stdin("(record { foo = 42 : nat })")
        .assert()
        .success()
        .stdout("(42 : nat)\n");
}

#[test]
fn variant_chained_optional_extracts_nested_field() {
    // .kind.Transfer?.amount — skips chain if tag doesn't match
    cq()
        .args([".kind.Transfer?.amount"])
        .write_stdin("(record { kind = variant { Transfer = record { amount = 100 : nat } } })")
        .assert()
        .success()
        .stdout("(100 : nat)\n");
}

#[test]
fn variant_chained_optional_empty_on_mismatch() {
    cq()
        .args([".Transfer?.amount"])
        .write_stdin("(variant { Pending })")
        .assert()
        .success()
        .stdout("");
}

#[test]
fn variant_round_trip_via_tag_access() {
    let extracted = cq()
        .args([".Ok"])
        .write_stdin("(variant { Ok = \"world\" })")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    // piping extracted payload through cq identity is lossless
    cq()
        .args(["."])
        .write_stdin(extracted.as_slice())
        .assert()
        .success()
        .stdout("(\"world\")\n");
}

// --- Legacy round-trip tests (kept for regression) ---

#[test]
fn round_trip_text_via_hex_preserves_hash() {
    // Binary Candid encodes field names as hashes; round-tripping through hex
    // without a .did schema replaces names with their numeric hash values.
    let output = cq()
        .arg("--output")
        .arg("hex")
        .write_stdin("(record { foo = 1 : nat })")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let hex_str = String::from_utf8(output).unwrap();
    // Hash of "foo" is 5_097_222; no schema means field name is lost.
    cq()
        .arg("--input-format")
        .arg("hex")
        .write_stdin(hex_str.trim())
        .assert()
        .success()
        .stdout("(record { 5_097_222 = 1 : nat })\n");
}

#[test]
fn round_trip_text_to_text_is_identity() {
    // cq | cq in text mode is lossless.
    let first = cq()
        .write_stdin("(opt (42 : nat32))")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let first_str = String::from_utf8(first).unwrap();
    cq()
        .write_stdin(first_str.trim())
        .assert()
        .success()
        .stdout(format!("{}\n", first_str.trim()));
}

// --- Slice 7: construction syntax ---

#[test]
fn construct_record_projects_fields() {
    cq()
        .args(["{x: .a, y: .b}"])
        .write_stdin("(record { a = 1 : nat; b = 2 : nat })")
        .assert()
        .success()
        .stdout("(record { x = 1 : nat; y = 2 : nat })\n");
}

#[test]
fn construct_record_empty() {
    cq()
        .args(["{}"])
        .write_stdin("(42 : nat)")
        .assert()
        .success()
        .stdout("(record {})\n");
}

#[test]
fn construct_record_with_identity() {
    cq()
        .args(["{val: .}"])
        .write_stdin("(42 : nat)")
        .assert()
        .success()
        .stdout("(record { val = 42 : nat })\n");
}

#[test]
fn construct_vec_from_fields() {
    cq()
        .args(["[.a, .b, .c]"])
        .write_stdin("(record { a = 1 : nat; b = 2 : nat; c = 3 : nat })")
        .assert()
        .success()
        .stdout("(vec { 1 : nat; 2 : nat; 3 : nat })\n");
}

#[test]
fn construct_vec_empty() {
    cq()
        .args(["[]"])
        .write_stdin("(42 : nat)")
        .assert()
        .success()
        .stdout("(vec {})\n");
}

#[test]
fn construct_variant_with_payload() {
    cq()
        .args(["variant { Ok = . }"])
        .write_stdin("(42 : nat)")
        .assert()
        .success()
        .stdout("(variant { Ok = 42 : nat })\n");
}

#[test]
fn construct_variant_null_payload() {
    cq()
        .args(["variant { Pending }"])
        .write_stdin("(null)")
        .assert()
        .success()
        .stdout("(variant { Pending })\n");
}

#[test]
fn construct_principal_literal() {
    cq()
        .args(["principal \"aaaaa-aa\""])
        .write_stdin("(null)")
        .assert()
        .success()
        .stdout("(principal \"aaaaa-aa\")\n");
}

#[test]
fn construct_principal_invalid_errors() {
    cq()
        .args(["principal \"not-valid-principal\""])
        .write_stdin("(null)")
        .assert()
        .failure()
        .stderr(contains("invalid principal"));
}

#[test]
fn construct_blob_string_literal() {
    cq()
        .args(["blob \"hello\""])
        .write_stdin("(null)")
        .assert()
        .success()
        .stdout("(blob \"hello\")\n");
}

#[test]
fn construct_blob_hex_escapes() {
    cq()
        .args(["blob \"\\00\\01\\02\""])
        .write_stdin("(null)")
        .assert()
        .success()
        .stdout("(blob \"\\00\\01\\02\")\n");
}

#[test]
fn construct_blob_hex_from_text_field() {
    cq()
        .args(["blob_hex(.hex)"])
        .write_stdin("(record { hex = \"deadbeef\" })")
        .assert()
        .success()
        .stdout("(blob \"\\de\\ad\\be\\ef\")\n");
}

#[test]
fn construct_blob_hex_invalid_errors() {
    cq()
        .args(["blob_hex(.hex)"])
        .write_stdin("(record { hex = \"not-hex\" })")
        .assert()
        .failure()
        .stderr(contains("invalid hex"));
}

#[test]
fn ascribe_nat32() {
    cq()
        .args([". : nat32"])
        .write_stdin("(42 : nat)")
        .assert()
        .success()
        .stdout("(42 : nat32)\n");
}

#[test]
fn ascribe_nat8_out_of_range_errors() {
    cq()
        .args([". : nat8"])
        .write_stdin("(256 : nat)")
        .assert()
        .failure()
        .stderr(contains("out of range"));
}

#[test]
fn ascribe_in_record_field() {
    cq()
        .args(["{amount: .bal : nat32}"])
        .write_stdin("(record { bal = 42 : nat })")
        .assert()
        .success()
        .stdout("(record { amount = 42 : nat32 })\n");
}

#[test]
fn construct_record_round_trip_through_field_access() {
    // Build a record then access a field to verify fidelity
    cq()
        .args(["{x: .a} | .x"])
        .write_stdin("(record { a = 99 : nat })")
        .assert()
        .success()
        .stdout("(99 : nat)\n");
}

// --- Slice 6: opt handling ---

#[test]
fn opt_field_access_returns_wrapped_value() {
    // .field on an opt field returns the opt-wrapped value (lossless)
    cq()
        .args([".x"])
        .write_stdin("(record { x = opt 42 : opt nat })")
        .assert()
        .success()
        .stdout("(opt (42 : nat))\n");
}

#[test]
fn opt_unwrap_some_returns_inner() {
    cq()
        .args([".x?"])
        .write_stdin("(record { x = opt 42 : opt nat })")
        .assert()
        .success()
        .stdout("(42 : nat)\n");
}

#[test]
fn opt_unwrap_none_produces_no_output() {
    cq()
        .args([".x?"])
        .write_stdin("(record { x = null : opt nat })")
        .assert()
        .success()
        .stdout("");
}

#[test]
fn opt_assert_some_returns_inner() {
    cq()
        .args([".x!"])
        .write_stdin("(record { x = opt 42 : opt nat })")
        .assert()
        .success()
        .stdout("(42 : nat)\n");
}

#[test]
fn opt_assert_none_errors() {
    cq()
        .args([".x!"])
        .write_stdin("(record { x = null : opt nat })")
        .assert()
        .failure()
        .stderr(contains("None"));
}

#[test]
fn opt_alt_some_returns_inner() {
    // .x // none — when x is Some, return the inner value (unwrapped)
    cq()
        .args([".x // none"])
        .write_stdin("(record { x = opt 42 : opt nat })")
        .assert()
        .success()
        .stdout("(42 : nat)\n");
}

#[test]
fn opt_alt_none_returns_fallback() {
    // .x // none — when x is None, return the `none` literal
    cq()
        .args([".x // none"])
        .write_stdin("(record { x = null : opt nat })")
        .assert()
        .success()
        .stdout("(null)\n");
}

#[test]
fn opt_some_constructor_wraps_value() {
    cq()
        .args(["some(.)"])
        .write_stdin("(42 : nat)")
        .assert()
        .success()
        .stdout("(opt (42 : nat))\n");
}

#[test]
fn opt_none_literal() {
    cq()
        .args(["none"])
        .write_stdin("(42 : nat)")
        .assert()
        .success()
        .stdout("(null)\n");
}

#[test]
fn opt_chained_unwrap_some_accesses_inner_field() {
    // .address?.city — when address is Some, unwrap and access city
    cq()
        .args([".address?.city"])
        .write_stdin("(record { address = opt record { city = \"NYC\" } })")
        .assert()
        .success()
        .stdout("(\"NYC\")\n");
}

#[test]
fn opt_chained_unwrap_none_produces_no_output() {
    // .address?.city — when address is None, produces nothing
    cq()
        .args([".address?.city"])
        .write_stdin("(record { address = null : opt record {} })")
        .assert()
        .success()
        .stdout("");
}

#[test]
fn opt_some_round_trip() {
    // some(.) wraps a value; .? unwraps it back
    let wrapped = cq()
        .args(["some(.)"])
        .write_stdin("(\"hello\")")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let wrapped_str = String::from_utf8(wrapped).unwrap();
    // Use .? to unwrap the standalone opt value
    cq()
        .args([".?"])
        .write_stdin(wrapped_str.trim())
        .assert()
        .success()
        .stdout("(\"hello\")\n");
}

// --- Slice 8: arithmetic, comparison, select, booleans ---

#[test]
fn arith_add_nat() {
    cq()
        .args([".a + .b"])
        .write_stdin("(record { a = 10 : nat; b = 3 : nat })")
        .assert()
        .success()
        .stdout("(13 : nat)\n");
}

#[test]
fn arith_sub_nat_positive_result() {
    cq()
        .args([".a - .b"])
        .write_stdin("(record { a = 10 : nat; b = 3 : nat })")
        .assert()
        .success()
        .stdout("(7 : nat)\n");
}

#[test]
fn arith_sub_nat_negative_result_widens_to_int() {
    cq()
        .args([".a - .b"])
        .write_stdin("(record { a = 3 : nat; b = 10 : nat })")
        .assert()
        .success()
        .stdout("(-7 : int)\n");
}

#[test]
fn arith_mul_nat() {
    cq()
        .args([".a * .b"])
        .write_stdin("(record { a = 10 : nat; b = 3 : nat })")
        .assert()
        .success()
        .stdout("(30 : nat)\n");
}

#[test]
fn arith_div_nat() {
    cq()
        .args([".a / .b"])
        .write_stdin("(record { a = 10 : nat; b = 3 : nat })")
        .assert()
        .success()
        .stdout("(3 : nat)\n");
}

#[test]
fn arith_rem_nat() {
    cq()
        .args([".a % .b"])
        .write_stdin("(record { a = 10 : nat; b = 3 : nat })")
        .assert()
        .success()
        .stdout("(1 : nat)\n");
}

#[test]
fn arith_add_literal() {
    cq()
        .args([". + 5"])
        .write_stdin("(10 : nat)")
        .assert()
        .success()
        .stdout("(15 : nat)\n");
}

#[test]
fn arith_sized_integer_no_overflow_during_eval() {
    // nat8(200) + 100 = 300: no error during eval; result is nat, not nat8
    cq()
        .args([". + 100"])
        .write_stdin("(200 : nat8)")
        .assert()
        .success()
        .stdout("(300 : nat)\n");
}

#[test]
fn arith_sized_integer_overflow_at_ascription() {
    // Overflow only errors when ascribed back to nat8
    cq()
        .args([". + 100 : nat8"])
        .write_stdin("(200 : nat8)")
        .assert()
        .failure()
        .stderr(contains("out of range for nat8"));
}

#[test]
fn arith_float_int_mixing_errors() {
    cq()
        .args([".a + .b"])
        .write_stdin("(record { a = 1.5 : float64; b = 3 : nat })")
        .assert()
        .failure()
        .stderr(contains("cannot mix float and integer"));
}

#[test]
fn cmp_eq_false() {
    cq()
        .args([".a == .b"])
        .write_stdin("(record { a = 10 : nat; b = 3 : nat })")
        .assert()
        .success()
        .stdout("(false)\n");
}

#[test]
fn cmp_eq_true() {
    cq()
        .args([".a == .b"])
        .write_stdin("(record { a = 5 : nat; b = 5 : nat })")
        .assert()
        .success()
        .stdout("(true)\n");
}

#[test]
fn cmp_ne() {
    cq()
        .args([".a != .b"])
        .write_stdin("(record { a = 10 : nat; b = 3 : nat })")
        .assert()
        .success()
        .stdout("(true)\n");
}

#[test]
fn cmp_lt() {
    cq()
        .args([". < 5"])
        .write_stdin("(3 : nat)")
        .assert()
        .success()
        .stdout("(true)\n");
}

#[test]
fn cmp_gt() {
    cq()
        .args([". > 5"])
        .write_stdin("(10 : nat)")
        .assert()
        .success()
        .stdout("(true)\n");
}

#[test]
fn cmp_le_equal() {
    cq()
        .args([". <= 5"])
        .write_stdin("(5 : nat)")
        .assert()
        .success()
        .stdout("(true)\n");
}

#[test]
fn cmp_ge_greater() {
    cq()
        .args([". >= 3"])
        .write_stdin("(5 : nat)")
        .assert()
        .success()
        .stdout("(true)\n");
}

#[test]
fn cmp_signed_unsigned_mixed() {
    // signed int(-5) < unsigned nat(3)
    cq()
        .args([".a < .b"])
        .write_stdin("(record { a = -5 : int; b = 3 : nat })")
        .assert()
        .success()
        .stdout("(true)\n");
}

#[test]
fn select_passes_matching_value() {
    cq()
        .args(["select(. > 3)"])
        .write_stdin("(5 : nat)")
        .assert()
        .success()
        .stdout("(5 : nat)\n");
}

#[test]
fn select_filters_non_matching_value() {
    cq()
        .args(["select(. > 3)"])
        .write_stdin("(2 : nat)")
        .assert()
        .success()
        .stdout("");
}

#[test]
fn select_on_stream_filters_partial() {
    // Two values: 5 passes, 2 is filtered out
    let out = cq()
        .args(["select(. > 3)"])
        .write_stdin("(5 : nat)\n(2 : nat)")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(String::from_utf8(out).unwrap(), "(5 : nat)\n");
}

#[test]
fn bool_not_true() {
    cq()
        .args(["not ."])
        .write_stdin("(true)")
        .assert()
        .success()
        .stdout("(false)\n");
}

#[test]
fn bool_not_false() {
    cq()
        .args(["not ."])
        .write_stdin("(false)")
        .assert()
        .success()
        .stdout("(true)\n");
}

#[test]
fn bool_and_both_true() {
    cq()
        .args(["true and true"])
        .write_stdin("(null)")
        .assert()
        .success()
        .stdout("(true)\n");
}

#[test]
fn bool_and_one_false() {
    cq()
        .args(["true and false"])
        .write_stdin("(null)")
        .assert()
        .success()
        .stdout("(false)\n");
}

#[test]
fn bool_or_one_true() {
    cq()
        .args(["false or true"])
        .write_stdin("(null)")
        .assert()
        .success()
        .stdout("(true)\n");
}

#[test]
fn bool_or_both_false() {
    cq()
        .args(["false or false"])
        .write_stdin("(null)")
        .assert()
        .success()
        .stdout("(false)\n");
}

#[test]
fn bool_compound_and_or() {
    // (. > 3) and (. < 10)
    cq()
        .args([". > 3 and . < 10"])
        .write_stdin("(5 : nat)")
        .assert()
        .success()
        .stdout("(true)\n");
}

#[test]
fn select_with_compound_predicate() {
    cq()
        .args(["select(. > 3 and . < 10)"])
        .write_stdin("(5 : nat)")
        .assert()
        .success()
        .stdout("(5 : nat)\n");
}

// --- Slice 9: match and tag ---

#[test]
fn match_dispatches_to_correct_arm() {
    cq()
        .args(["match { Transfer = . ; Receive = 0 }"])
        .write_stdin("(variant { Transfer = \"hello\" })")
        .assert()
        .success()
        .stdout("(\"hello\")\n");
}

#[test]
fn match_payload_bound_as_dot() {
    cq()
        .args(["match { Ok = . ; Err = \"error\" }"])
        .write_stdin("(variant { Ok = 42 : nat })")
        .assert()
        .success()
        .stdout("(42 : nat)\n");
}

#[test]
fn match_default_arm_catches_unmatched() {
    cq()
        .args(["match { Transfer = \"transfer\" ; _ = \"other\" }"])
        .write_stdin("(variant { Mint = null })")
        .assert()
        .success()
        .stdout("(\"other\")\n");
}

#[test]
fn match_no_arm_no_default_errors() {
    cq()
        .args(["match { Transfer = . }"])
        .write_stdin("(variant { Receive = null })")
        .assert()
        .failure()
        .stderr(contains("no match arm"));
}

#[test]
fn match_null_payload_arm() {
    cq()
        .args(["match { Pending = \"pending\" ; _ = \"other\" }"])
        .write_stdin("(variant { Pending })")
        .assert()
        .success()
        .stdout("(\"pending\")\n");
}

#[test]
fn match_non_variant_errors() {
    cq()
        .args(["match { A = . }"])
        .write_stdin("(42 : nat)")
        .assert()
        .failure()
        .stderr(contains("variant"));
}

#[test]
fn tag_returns_active_tag_as_text() {
    cq()
        .args(["tag(.)"])
        .write_stdin("(variant { Transfer = 100 : nat })")
        .assert()
        .success()
        .stdout("(\"Transfer\")\n");
}

#[test]
fn tag_on_non_variant_errors() {
    cq()
        .args(["tag(.)"])
        .write_stdin("(42 : nat)")
        .assert()
        .failure()
        .stderr(contains("variant"));
}

#[test]
fn tag_on_nested_variant_field() {
    cq()
        .args(["tag(.status)"])
        .write_stdin("(record { status = variant { Active = null } })")
        .assert()
        .success()
        .stdout("(\"Active\")\n");
}

#[test]
fn tag_select_filter_stream() {
    cq()
        .args(["select(tag(.kind) == \"Transfer\")"])
        .write_stdin(
            "(record { kind = variant { Transfer = 100 : nat } })\
             (record { kind = variant { Receive = 50 : nat } })\
             (record { kind = variant { Transfer = 200 : nat } })",
        )
        .assert()
        .success()
        .stdout(
            "(record { kind = variant { Transfer = 100 : nat } })\n\
             (record { kind = variant { Transfer = 200 : nat } })\n",
        );
}

#[test]
fn match_body_expression_uses_payload() {
    cq()
        .args(["match { Transfer = . * 2 ; _ = 0 }"])
        .write_stdin("(variant { Transfer = 10 : nat })")
        .assert()
        .success()
        .stdout("(20 : nat)\n");
}

// --- Slice 10: if/elif/else, as $x, string interpolation ---

#[test]
fn if_true_branch() {
    cq()
        .args(["if .x then \"yes\" else \"no\" end"])
        .write_stdin("(record { x = true })")
        .assert()
        .success()
        .stdout("(\"yes\")\n");
}

#[test]
fn if_false_branch() {
    cq()
        .args(["if .x then \"yes\" else \"no\" end"])
        .write_stdin("(record { x = false })")
        .assert()
        .success()
        .stdout("(\"no\")\n");
}

#[test]
fn if_elif_else() {
    cq()
        .args(["if .x == 1 then \"one\" elif .x == 2 then \"two\" else \"other\" end"])
        .write_stdin("(record { x = 2 : nat })")
        .assert()
        .success()
        .stdout("(\"two\")\n");
}

#[test]
fn if_elif_else_default() {
    cq()
        .args(["if .x == 1 then \"one\" elif .x == 2 then \"two\" else \"other\" end"])
        .write_stdin("(record { x = 99 : nat })")
        .assert()
        .success()
        .stdout("(\"other\")\n");
}

#[test]
fn if_condition_not_bool_errors() {
    cq()
        .args(["if . then \"yes\" else \"no\" end"])
        .write_stdin("(42 : nat)")
        .assert()
        .failure()
        .stderr(contains("bool"));
}

#[test]
fn if_without_else_no_match_returns_empty() {
    cq()
        .args(["if .x then \"yes\" end"])
        .write_stdin("(record { x = false })")
        .assert()
        .success()
        .stdout("");
}

#[test]
fn var_bind_basic() {
    cq()
        .args([".foo as $x | {result: $x}"])
        .write_stdin("(record { foo = 42 : nat })")
        .assert()
        .success()
        .stdout("(record { result = 42 : nat })\n");
}

#[test]
fn var_bind_multiple() {
    cq()
        .args([".a as $a | .b as $b | $a + $b"])
        .write_stdin("(record { a = 10 : nat; b = 20 : nat })")
        .assert()
        .success()
        .stdout("(30 : nat)\n");
}

#[test]
fn var_bind_in_condition() {
    cq()
        .args([".val as $v | if $v == 1 then \"one\" else \"other\" end"])
        .write_stdin("(record { val = 1 : nat })")
        .assert()
        .success()
        .stdout("(\"one\")\n");
}

#[test]
fn var_ref_undefined_errors() {
    cq()
        .args(["$undefined"])
        .write_stdin("(42 : nat)")
        .assert()
        .failure()
        .stderr(contains("undefined variable"));
}

#[test]
fn string_interp_text_field() {
    cq()
        .args(["\"hello \\(.name)\""])
        .write_stdin("(record { name = \"world\" })")
        .assert()
        .success()
        .stdout("(\"hello world\")\n");
}

#[test]
fn string_interp_nat_field() {
    cq()
        .args(["\"count: \\(.n)\""])
        .write_stdin("(record { n = 42 : nat })")
        .assert()
        .success()
        .stdout("(\"count: 42\")\n");
}

#[test]
fn string_interp_prefix_and_suffix() {
    cq()
        .args(["\"[\\(.x)]\""])
        .write_stdin("(record { x = 7 : nat })")
        .assert()
        .success()
        .stdout("(\"[7]\")\n");
}

#[test]
fn string_interp_nested() {
    cq()
        .args(["\"outer \\(\"inner \\(.v)\")\""])
        .write_stdin("(record { v = 1 : nat })")
        .assert()
        .success()
        .stdout("(\"outer inner 1\")\n");
}

#[test]
fn string_interp_var() {
    cq()
        .args([".x as $v | \"val=\\($v)\""])
        .write_stdin("(record { x = 5 : nat })")
        .assert()
        .success()
        .stdout("(\"val=5\")\n");
}

// --- Slice 11: generic + conversion builtins ---

#[test]
fn length_vec() {
    cq()
        .args(["length"])
        .write_stdin("(vec { 1 : nat; 2 : nat; 3 : nat })")
        .assert()
        .success()
        .stdout("(3 : nat)\n");
}

#[test]
fn length_text() {
    cq()
        .args(["length"])
        .write_stdin("(\"hello\")")
        .assert()
        .success()
        .stdout("(5 : nat)\n");
}

#[test]
fn length_blob() {
    cq()
        .args(["length"])
        .write_stdin("(blob \"\\00\\01\\02\")")
        .assert()
        .success()
        .stdout("(3 : nat)\n");
}

#[test]
fn keys_record_sorted() {
    cq()
        .args(["keys"])
        .write_stdin("(record { b = 2 : nat; a = 1 : nat })")
        .assert()
        .success()
        .stdout("(vec { \"a\"; \"b\" })\n");
}

#[test]
fn values_record_sorted_by_key() {
    cq()
        .args(["values"])
        .write_stdin("(record { b = 2 : nat; a = 1 : nat })")
        .assert()
        .success()
        .stdout("(vec { 1 : nat; 2 : nat })\n");
}

#[test]
fn type_nat() {
    cq()
        .args(["type"])
        .write_stdin("(42 : nat)")
        .assert()
        .success()
        .stdout("(\"nat\")\n");
}

#[test]
fn type_record() {
    cq()
        .args(["type"])
        .write_stdin("(record { x = 1 : nat })")
        .assert()
        .success()
        .stdout("(\"record\")\n");
}

#[test]
fn type_variant() {
    cq()
        .args(["type"])
        .write_stdin("(variant { Ok = \"hi\" })")
        .assert()
        .success()
        .stdout("(\"variant\")\n");
}

#[test]
fn type_vec() {
    cq()
        .args(["type"])
        .write_stdin("(vec { 1 : nat })")
        .assert()
        .success()
        .stdout("(\"vec\")\n");
}

#[test]
fn has_field_present() {
    cq()
        .args(["has(\"x\")"])
        .write_stdin("(record { x = 1 : nat })")
        .assert()
        .success()
        .stdout("(true)\n");
}

#[test]
fn has_field_absent() {
    cq()
        .args(["has(\"y\")"])
        .write_stdin("(record { x = 1 : nat })")
        .assert()
        .success()
        .stdout("(false)\n");
}

#[test]
fn contains_text_found() {
    cq()
        .args(["contains(\"world\")"])
        .write_stdin("(\"hello world\")")
        .assert()
        .success()
        .stdout("(true)\n");
}

#[test]
fn contains_text_not_found() {
    cq()
        .args(["contains(\"xyz\")"])
        .write_stdin("(\"hello world\")")
        .assert()
        .success()
        .stdout("(false)\n");
}

#[test]
fn contains_vec_found() {
    cq()
        .args(["contains(2 : nat)"])
        .write_stdin("(vec { 1 : nat; 2 : nat; 3 : nat })")
        .assert()
        .success()
        .stdout("(true)\n");
}

#[test]
fn contains_vec_not_found() {
    cq()
        .args(["contains(99 : nat)"])
        .write_stdin("(vec { 1 : nat; 2 : nat; 3 : nat })")
        .assert()
        .success()
        .stdout("(false)\n");
}

#[test]
fn map_add_one() {
    cq()
        .args(["map(. + 1)"])
        .write_stdin("(vec { 1 : nat; 2 : nat; 3 : nat })")
        .assert()
        .success()
        .stdout("(vec { 2 : nat; 3 : nat; 4 : nat })\n");
}

#[test]
fn map_empty_vec() {
    cq()
        .args(["map(. + 1)"])
        .write_stdin("(vec {})")
        .assert()
        .success()
        .stdout("(vec {})\n");
}

#[test]
fn to_text_nat() {
    cq()
        .args(["to_text"])
        .write_stdin("(42 : nat)")
        .assert()
        .success()
        .stdout("(\"42\")\n");
}

#[test]
fn to_text_bool() {
    cq()
        .args(["to_text"])
        .write_stdin("(true)")
        .assert()
        .success()
        .stdout("(\"true\")\n");
}

#[test]
fn to_int_from_nat() {
    cq()
        .args(["to_int"])
        .write_stdin("(42 : nat)")
        .assert()
        .success()
        .stdout("(42 : int)\n");
}

#[test]
fn to_int_from_text() {
    cq()
        .args(["to_int"])
        .write_stdin("(\"-7\")")
        .assert()
        .success()
        .stdout("(-7 : int)\n");
}

#[test]
fn to_float_from_nat() {
    cq()
        .args(["to_float"])
        .write_stdin("(3 : nat)")
        .assert()
        .success()
        .stdout("(3.0 : float64)\n");
}

#[test]
fn to_principal_from_text() {
    cq()
        .args(["to_principal"])
        .write_stdin("(\"aaaaa-aa\")")
        .assert()
        .success()
        .stdout("(principal \"aaaaa-aa\")\n");
}

#[test]
fn to_hex_blob() {
    cq()
        .args(["to_hex"])
        .write_stdin("(blob \"\\00\\ff\")")
        .assert()
        .success()
        .stdout("(\"00ff\")\n");
}

#[test]
fn from_hex_text() {
    cq()
        .args(["from_hex"])
        .write_stdin("(\"00ff\")")
        .assert()
        .success()
        .stdout("(blob \"\\00\\ff\")\n");
}

#[test]
fn to_utf8_text() {
    cq()
        .args(["to_utf8"])
        .write_stdin("(\"hi\")")
        .assert()
        .success()
        .stdout("(blob \"hi\")\n");
}

#[test]
fn from_utf8_blob() {
    cq()
        .args(["from_utf8"])
        .write_stdin("(blob \"hi\")")
        .assert()
        .success()
        .stdout("(\"hi\")\n");
}

#[test]
fn is_some_on_opt_some() {
    cq()
        .args(["is_some"])
        .write_stdin("(opt (42 : nat))")
        .assert()
        .success()
        .stdout("(true)\n");
}

#[test]
fn is_some_on_opt_none() {
    cq()
        .args(["is_some"])
        .write_stdin("(null : opt nat)")
        .assert()
        .success()
        .stdout("(false)\n");
}

#[test]
fn is_none_on_opt_none() {
    cq()
        .args(["is_none"])
        .write_stdin("(null : opt nat)")
        .assert()
        .success()
        .stdout("(true)\n");
}

#[test]
fn is_none_on_opt_some() {
    cq()
        .args(["is_none"])
        .write_stdin("(opt (42 : nat))")
        .assert()
        .success()
        .stdout("(false)\n");
}

#[test]
fn principal_equality() {
    cq()
        .args([". == principal \"aaaaa-aa\""])
        .write_stdin("(principal \"aaaaa-aa\")")
        .assert()
        .success()
        .stdout("(true)\n");
}

#[test]
fn principal_inequality() {
    cq()
        .args([". != principal \"aaaaa-aa\""])
        .write_stdin("(principal \"2vxsx-fae\")")
        .assert()
        .success()
        .stdout("(true)\n");
}

// --- Slice 12: analysis builtins (sort, sort_by, group_by, unique) ---

#[test]
fn sort_nat_vec() {
    cq()
        .args(["sort"])
        .write_stdin("(vec { 3 : nat; 1 : nat; 2 : nat })")
        .assert()
        .success()
        .stdout("(vec { 1 : nat; 2 : nat; 3 : nat })\n");
}

#[test]
fn sort_text_vec() {
    cq()
        .args(["sort"])
        .write_stdin("(vec { \"banana\"; \"apple\"; \"cherry\" })")
        .assert()
        .success()
        .stdout("(vec { \"apple\"; \"banana\"; \"cherry\" })\n");
}

#[test]
fn sort_empty_vec() {
    cq()
        .args(["sort"])
        .write_stdin("(vec {})")
        .assert()
        .success()
        .stdout("(vec {})\n");
}

#[test]
fn sort_already_sorted() {
    cq()
        .args(["sort"])
        .write_stdin("(vec { 1 : nat; 2 : nat; 3 : nat })")
        .assert()
        .success()
        .stdout("(vec { 1 : nat; 2 : nat; 3 : nat })\n");
}

#[test]
fn sort_mixed_types_errors() {
    cq()
        .args(["sort"])
        .write_stdin("(vec { 1 : nat; \"text\" })")
        .assert()
        .failure()
        .stderr(contains("incompatible types"));
}

#[test]
fn sort_by_field() {
    cq()
        .args(["sort_by(.age)"])
        .write_stdin(
            "(vec { record { name = \"bob\"; age = 30 : nat }; record { name = \"alice\"; age = 25 : nat } })",
        )
        .assert()
        .success()
        .stdout(
            "(\n  vec {\n    record { age = 25 : nat; name = \"alice\" };\n    record { age = 30 : nat; name = \"bob\" };\n  },\n)\n",
        );
}

#[test]
fn sort_by_text_field() {
    cq()
        .args(["sort_by(.name)"])
        .write_stdin(
            "(vec { record { name = \"charlie\" }; record { name = \"alice\" }; record { name = \"bob\" } })",
        )
        .assert()
        .success()
        .stdout(
            "(\n  vec {\n    record { name = \"alice\" };\n    record { name = \"bob\" };\n    record { name = \"charlie\" };\n  },\n)\n",
        );
}

#[test]
fn sort_by_empty_vec() {
    cq()
        .args(["sort_by(.x)"])
        .write_stdin("(vec {})")
        .assert()
        .success()
        .stdout("(vec {})\n");
}

#[test]
fn group_by_tag() {
    cq()
        .args(["group_by(tag(.))"])
        .write_stdin(
            "(vec { variant { Ok = 1 : nat }; variant { Err = \"bad\" }; variant { Ok = 2 : nat } })",
        )
        .assert()
        .success()
        .stdout(
            "(\n  vec {\n    vec { variant { Ok = 1 : nat }; variant { Ok = 2 : nat } };\n    vec { variant { Err = \"bad\" } };\n  },\n)\n",
        );
}

#[test]
fn group_by_field() {
    cq()
        .args(["group_by(.kind)"])
        .write_stdin(
            "(vec { record { kind = \"a\" }; record { kind = \"b\" }; record { kind = \"a\" } })",
        )
        .assert()
        .success()
        .stdout(
            "(\n  vec {\n    vec { record { kind = \"a\" }; record { kind = \"a\" } };\n    vec { record { kind = \"b\" } };\n  },\n)\n",
        );
}

#[test]
fn group_by_empty_vec() {
    cq()
        .args(["group_by(.x)"])
        .write_stdin("(vec {})")
        .assert()
        .success()
        .stdout("(vec {})\n");
}

#[test]
fn unique_deduplicates() {
    cq()
        .args(["unique"])
        .write_stdin("(vec { 1 : nat; 2 : nat; 1 : nat; 3 : nat; 2 : nat })")
        .assert()
        .success()
        .stdout("(vec { 1 : nat; 2 : nat; 3 : nat })\n");
}

#[test]
fn unique_preserves_first_occurrence_order() {
    cq()
        .args(["unique"])
        .write_stdin("(vec { \"c\"; \"a\"; \"b\"; \"a\"; \"c\" })")
        .assert()
        .success()
        .stdout("(vec { \"c\"; \"a\"; \"b\" })\n");
}

#[test]
fn unique_empty_vec() {
    cq()
        .args(["unique"])
        .write_stdin("(vec {})")
        .assert()
        .success()
        .stdout("(vec {})\n");
}

#[test]
fn unique_no_duplicates() {
    cq()
        .args(["unique"])
        .write_stdin("(vec { 1 : nat; 2 : nat; 3 : nat })")
        .assert()
        .success()
        .stdout("(vec { 1 : nat; 2 : nat; 3 : nat })\n");
}

// --- Slice 13: --did schema loader ---

#[test]
fn did_without_schema_shows_hash() {
    // Binary-decoded record without --did renders field as numeric hash
    cq()
        .args(["--input-format", "hex"])
        .write_stdin("4449444c016c01868eb7027d01002a")
        .assert()
        .success()
        .stdout("(record { 5_097_222 = 42 : nat })\n");
}

#[test]
fn did_with_schema_resolves_record_field() {
    // Binary-decoded record with --did renders original field name
    cq()
        .args(["--input-format", "hex", "--did", "tests/fixtures/schema1.did"])
        .write_stdin("4449444c016c01868eb7027d01002a")
        .assert()
        .success()
        .stdout("(record { foo = 42 : nat })\n");
}

#[test]
fn did_without_schema_shows_variant_hash() {
    // Binary-decoded variant without --did renders tag as numeric hash
    cq()
        .args(["--input-format", "hex"])
        .write_stdin("4449444c016b01bc8a017d0100002a")
        .assert()
        .success()
        .stdout("(variant { 17_724 = 42 : nat })\n");
}

#[test]
fn did_with_schema_resolves_variant_tag() {
    // Binary-decoded variant with --did renders original tag name
    cq()
        .args(["--input-format", "hex", "--did", "tests/fixtures/schema1.did"])
        .write_stdin("4449444c016b01bc8a017d0100002a")
        .assert()
        .success()
        .stdout("(variant { Ok = 42 : nat })\n");
}

#[test]
fn did_multiple_flags_union_schemas() {
    // --did can be specified multiple times; names from both files are resolved
    cq()
        .args([
            "--input-format",
            "hex",
            "--did",
            "tests/fixtures/schema1.did",
            "--did",
            "tests/fixtures/schema2.did",
        ])
        .write_stdin("4449444c016c01dbe3aa027e010001")
        .assert()
        .success()
        .stdout("(record { baz = true })\n");
}

#[test]
fn did_file_not_found_error() {
    cq()
        .args(["--did", "nonexistent.did"])
        .write_stdin("(42 : nat)")
        .assert()
        .failure()
        .stderr(contains("failed to read nonexistent.did"));
}

#[test]
fn did_file_malformed_error() {
    cq()
        .args(["--did", "tests/fixtures/bad.did"])
        .write_stdin("(42 : nat)")
        .assert()
        .failure()
        .stderr(contains("failed to parse tests/fixtures/bad.did"));
}

// --- Slice 14: idl_hash auto-resolution + .[hash] numeric fallback ---

#[test]
fn hash_auto_resolve_named_field_no_did() {
    // .foo on a hash-keyed binary record (no --did) works via idl_hash auto-resolution
    // idl_hash("foo") = 5_097_222 (the hash in the binary record)
    cq()
        .args(["--input-format", "hex", ".foo"])
        .write_stdin("4449444c016c01868eb7027d01002a")
        .assert()
        .success()
        .stdout("(42 : nat)\n");
}

#[test]
fn hash_numeric_access_on_hash_keyed_record() {
    // .[5097222] accesses by raw hash on a binary-decoded record
    cq()
        .args(["--input-format", "hex", ".[5097222]"])
        .write_stdin("4449444c016c01868eb7027d01002a")
        .assert()
        .success()
        .stdout("(42 : nat)\n");
}

#[test]
fn hash_numeric_access_on_named_field_record() {
    // .[n] also works when the field has a Named label (text input)
    // idl_hash("foo") = 5097222
    cq()
        .args([".[5097222]"])
        .write_stdin("(record { foo = 42 : nat })")
        .assert()
        .success()
        .stdout("(42 : nat)\n");
}

#[test]
fn hash_numeric_access_with_did() {
    // .[n] still works when --did is provided (schema doesn't affect in-memory IDLValue labels)
    cq()
        .args([
            "--input-format",
            "hex",
            "--did",
            "tests/fixtures/schema1.did",
            ".[5097222]",
        ])
        .write_stdin("4449444c016c01868eb7027d01002a")
        .assert()
        .success()
        .stdout("(42 : nat)\n");
}

#[test]
fn hash_numeric_access_not_found_error() {
    // .[n] with a hash not present in the record gives a clear error
    cq()
        .args([".[9999999]"])
        .write_stdin("(record { foo = 42 : nat })")
        .assert()
        .failure()
        .stderr(contains("no field with hash 9999999"));
}

#[test]
fn hash_named_field_wrong_name_fails() {
    // Querying .old_name after a field was renamed fails loudly (schema drift caught)
    cq()
        .args(["--input-format", "hex", ".renamed_field"])
        .write_stdin("4449444c016c01868eb7027d01002a")
        .assert()
        .failure()
        .stderr(contains("unknown field"));
}

#[test]
fn hash_named_and_numeric_equivalent() {
    // .bar and .[idl_hash("bar")] produce the same result on a text-encoded record
    // idl_hash("bar") can be computed; we verify via chained queries that they're consistent
    cq()
        .args([".foo"])
        .write_stdin("(record { foo = 99 : nat; bar = 1 : nat })")
        .assert()
        .success()
        .stdout("(99 : nat)\n");
}

// --- Slice 15: did-you-mean error messages ---

#[test]
fn did_you_mean_close_field_name() {
    // "fo" is close to "foo" → did-you-mean suggestion
    cq()
        .args([".fo"])
        .write_stdin("(record { foo = 1 : nat })")
        .assert()
        .failure()
        .stderr(contains("did you mean 'foo'"));
}

#[test]
fn did_you_mean_no_suggestion_far_field() {
    // "xyz" is not close to any of foo/bar → no suggestion, but lists fields
    cq()
        .args([".xyz"])
        .write_stdin("(record { foo = 1 : nat; bar = 2 : nat })")
        .assert()
        .failure()
        .stderr(contains("unknown field 'xyz'"))
        .stderr(contains("bar"))
        .stderr(contains("foo"));
}

#[test]
fn unknown_field_lists_sorted_fields() {
    // Available fields are listed in sorted order
    cq()
        .args([".missing"])
        .write_stdin("(record { zebra = 1 : nat; apple = 2 : nat; mango = 3 : nat })")
        .assert()
        .failure()
        .stderr(contains("apple, mango, zebra"));
}

#[test]
fn match_no_arm_lists_arms() {
    // When no match arm fires, list all defined arms
    cq()
        .args(["match { Send = 1; Receive = 2 }"])
        .write_stdin("(variant { Transfer = 42 : nat })")
        .assert()
        .failure()
        .stderr(contains("Transfer"))
        .stderr(contains("Receive"))
        .stderr(contains("Send"));
}

#[test]
fn match_no_arm_did_you_mean() {
    // Typo in arm name → did-you-mean suggestion (no default arm so it errors)
    cq()
        .args(["match { Transfr = 1 }"])
        .write_stdin("(variant { Transfer = 42 : nat })")
        .assert()
        .failure()
        .stderr(contains("Transfer"))
        .stderr(contains("did you mean 'Transfr'"));
}

#[test]
fn arith_type_error_names_operator() {
    // '+' on non-numeric gives the operator name in the error
    cq()
        .args([r#". + 1"#])
        .write_stdin(r#"("hello")"#)
        .assert()
        .failure()
        .stderr(contains("'+'"))
        .stderr(contains("text"));
}

#[test]
fn cmp_type_error_names_operator() {
    // '==' on non-numeric non-text gives the operator name in the error
    cq()
        .args([". == true"])
        .write_stdin("(42 : nat)")
        .assert()
        .failure()
        .stderr(contains("'=='"))
        .stderr(contains("bool"));
}

#[test]
fn cmp_mixed_types_error() {
    // Comparing text vs nat gives a type-mismatch error naming both types
    cq()
        .args([r#". == 1"#])
        .write_stdin(r#"("hello")"#)
        .assert()
        .failure()
        .stderr(contains("'=='"))
        .stderr(contains("text"));
}
