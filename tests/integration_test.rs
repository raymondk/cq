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
