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

// --- Round-trip tests ---

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
