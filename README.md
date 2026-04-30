# cq

`cq` is **jq for Candid** — a small, offline CLI for inspecting and querying
[Candid](https://github.com/dfinity/candid), the IDL used on the Internet
Computer.

It reads Candid values from stdin (text, hex, or binary), applies a
jq-flavoured query, and writes the result back out (text, hex, or binary). It
links the `dfinity/candid` Rust crate directly — there is no shell-out to
`didc` and no network access. Schema fetching is a separate concern; pair `cq`
with `icp-cli` and `idl2json`.

> **Status:** in development. The query language and CLI are implemented in
> vertical slices; see the commit history for what has landed. The behaviour
> documented below is covered by the integration tests in `tests/`.

## Install

```sh
cargo build --release
# binary: target/release/cq
```

## Quick taste

```sh
$ echo '(record { foo = 42 : nat; bar = "hello" })' | cq '.foo'
(42 : nat)

$ echo '(variant { Ok = "hello" })' | cq '.Ok'
("hello")

$ echo '(vec { 1 : nat; 2 : nat; 3 : nat })' | cq 'map(. + 1)'
(vec { 2 : nat; 3 : nat; 4 : nat })
```

The bare invocation `cq` (no query) is identity — useful for pretty-printing
or format conversion:

```sh
$ echo '4449444c00017d2a' | cq --input-format hex
(42 : nat)
```

## Common use cases

### Reach into a record

```sh
$ echo '(record { a = record { b = record { c = 99 : nat } } })' | cq '.a.b.c'
(99 : nat)
```

Unknown fields fail loudly with a "did you mean…" hint:

```sh
$ echo '(record { foo = 1 : nat })' | cq '.fo'
cq: unknown field 'fo'; did you mean 'foo'?; available fields: foo
```

### Pull a payload out of a variant

```sh
$ echo '(variant { Ok = "hello" })' | cq '.Ok'
("hello")
```

`.Tag?` skips silently on tag mismatch (use it when streaming):

```sh
$ echo '(variant { Pending })' | cq '.Ok?'
# (no output, exit 0)
```

Chain it through nested records:

```sh
$ echo '(record { kind = variant { Transfer = record { amount = 100 : nat } } })' \
    | cq '.kind.Transfer?.amount'
(100 : nat)
```

### Index, slice, and explode vecs

```sh
$ echo '(vec { 1 : nat; 2 : nat; 3 : nat; 4 : nat })' | cq '.[0]'
(1 : nat)

$ echo '(vec { 1 : nat; 2 : nat; 3 : nat; 4 : nat })' | cq '.[1:3]'
(vec { 2 : nat; 3 : nat })

$ echo '(vec { 1 : nat; 2 : nat; 3 : nat })' | cq '.[]'
(1 : nat)
(2 : nat)
(3 : nat)
```

### Filter a stream

`select(p)` passes values where `p` is true and drops the rest. It composes
with `tag(.)` to filter a stream of variants by arm:

```sh
$ printf '%s' \
    '(record { kind = variant { Transfer = 100 : nat } })' \
    '(record { kind = variant { Receive  = 50  : nat } })' \
    '(record { kind = variant { Transfer = 200 : nat } })' \
  | cq 'select(tag(.kind) == "Transfer")'
(record { kind = variant { Transfer = 100 : nat } })
(record { kind = variant { Transfer = 200 : nat } })
```

Use `-e` / `--exit-status` to fail with exit code 1 when nothing passes:

```sh
$ echo '(vec {})' | cq -e '.[]'
# exit 1
```

### Project / reshape with construction syntax

```sh
$ echo '(record { a = 1 : nat; b = 2 : nat })' | cq '{x: .a, y: .b}'
(record { x = 1 : nat; y = 2 : nat })

$ echo '(record { a = 1 : nat; b = 2 : nat; c = 3 : nat })' | cq '[.a, .b, .c]'
(vec { 1 : nat; 2 : nat; 3 : nat })

$ echo '(42 : nat)' | cq 'variant { Ok = . }'
(variant { Ok = 42 : nat })
```

### Dispatch on variant tags with `match`

```sh
$ echo '(variant { Transfer = 10 : nat })' | cq 'match { Transfer = . * 2 ; _ = 0 }'
(20 : nat)
```

### Opt handling

`.field` on an opt field returns the opt-wrapped value, untouched. The
postfix operators choose what to do with `None`:

| form        | on `Some(x)` | on `None`            |
|-------------|--------------|----------------------|
| `.x`        | `opt x`      | `null` (preserved)   |
| `.x?`       | `x`          | empty (no output)    |
| `.x!`       | `x`          | error                |
| `.x // d`   | `x`          | `d`                  |

```sh
$ echo '(record { x = opt 42 : opt nat })' | cq '.x?'
(42 : nat)

$ echo '(record { x = null : opt nat })' | cq '.x // none'
(null)

$ echo '(record { address = opt record { city = "NYC" } })' | cq '.address?.city'
("NYC")
```

### Arithmetic & comparisons

Numbers evaluate as bigints during the query and only get re-typed at emit
time. Subtraction widens to `int` if it goes negative; sized-int overflow is
caught at the ascription boundary, not mid-expression.

```sh
$ echo '(record { a = 10 : nat; b = 3 : nat })' | cq '.a + .b'
(13 : nat)

$ echo '(record { a = 3 : nat; b = 10 : nat })' | cq '.a - .b'
(-7 : int)

$ echo '(200 : nat8)' | cq '. + 100'
(300 : nat)

$ echo '(200 : nat8)' | cq '. + 100 : nat8'
cq: value "300" is out of range for nat8
```

### Conversion / inspection builtins

```sh
$ echo '(vec { 1 : nat; 2 : nat; 3 : nat })' | cq 'length'
(3 : nat)

$ echo '(record { b = 2 : nat; a = 1 : nat })' | cq 'keys'
(vec { "a"; "b" })

$ printf '%s' '(blob "\00\ff")' | cq 'to_hex'
("00ff")

$ echo '("aaaaa-aa")' | cq 'to_principal'
(principal "aaaaa-aa")

$ echo '(record { name = "world" })' | cq '"hello \(.name)"'
("hello world")
```

Available builtins include `length`, `keys`, `values`, `type`, `has`,
`select`, `map`, `contains`, `tag`, `match`, `is_some`/`is_none`, `some`/
`none`, `to_text`/`to_int`/`to_float`/`to_principal`, `to_hex`/`from_hex`,
`to_utf8`/`from_utf8`, `sort`/`sort_by`/`group_by`/`unique`, plus the boolean
keywords `and`, `or`, `not`.

### Sort, group, dedupe

```sh
$ echo '(vec { 3 : nat; 1 : nat; 2 : nat })' | cq 'sort'
(vec { 1 : nat; 2 : nat; 3 : nat })

$ echo '(vec { record { name = "bob"; age = 30 : nat };
              record { name = "alice"; age = 25 : nat } })' \
    | cq 'sort_by(.age)'
# alice (25) first, then bob (30)

$ echo '(vec { 1 : nat; 2 : nat; 1 : nat; 3 : nat; 2 : nat })' | cq 'unique'
(vec { 1 : nat; 2 : nat; 3 : nat })
```

### Decode binary Candid (with or without a schema)

`cq` autodetects text / hex / binary on stdin. Without a schema, binary-
decoded records show field hashes:

```sh
$ printf '4449444c016c01868eb7027d01002a' | cq --input-format hex
(record { 5_097_222 = 42 : nat })
```

Pass a `.did` schema to recover names — `--did` may be repeated:

```sh
$ printf '4449444c016c01868eb7027d01002a' \
    | cq --input-format hex --did tests/fixtures/schema1.did
(record { foo = 42 : nat })
```

You don't always need the schema: writing `.foo` triggers `idl_hash`-based
auto-resolution, and `.[5097222]` is the explicit numeric fallback when only
the hash is known:

```sh
$ printf '4449444c016c01868eb7027d01002a' | cq --input-format hex '.foo'
(42 : nat)
```

### Compose with `icp-cli`

`icp-cli` produces text Candid; pipe it straight into `cq`:

```sh
$ icp canister call my_canister get_balance '(principal "...")' \
    | cq '.balance'
```

For binary results (e.g. `--output raw`) round-trip through hex:

```sh
$ icp canister call --output raw my_canister get_record '()' \
    | cq --input-format hex --did canister.did '.foo'
```

### Format conversion

Text ↔ hex ↔ bin is fully round-trippable in text mode (binary loses field
names without a schema). Use `--output` to convert:

```sh
$ echo '(42 : nat)' | cq --output hex
4449444c00017d2a

$ echo '(42 : nat)' | cq --output bin | xxd
00000000: 4449 444c 0001 7d2a                      DIDL..}*
```

## Options

```
cq [QUERY] [OPTIONS]

  QUERY                       Query expression (default: identity, .)

  --input-format FORMAT       candid | hex | bin     (default: auto-detect)
  --output FORMAT             candid | hex | bin     (default: candid)
                              (`text` is accepted as an alias for `candid`)
  --did PATH                  Path to a .did schema file. May be repeated.
  --color WHEN                auto | always | never  (default: auto;
                              respects NO_COLOR when auto)
  -c, --compact               Single-line output
      --blob-threshold N      Blobs longer than N bytes render as
                              blob_hex("...") (default: 64)
  -e, --exit-status           Exit 1 if no values were produced
```

## Design notes

- **Type-preserving.** `cq` operates on `IDLValue`/`IDLArgs`; sized ints,
  variants, opts, and nulls are all distinct. `cq | cq` is lossless for any
  query, in text mode.
- **Streaming.** A query produces 0/1/many values per input; results are
  flat-streamed (jq semantics). Multi-value text input (`(a)(b)` or
  newline-separated) and concatenated DIDL frames are both supported.
- **Strict by default.** `.foo` errors if the field is missing; `.foo?` is
  the explicit-tolerance form. Same for variant arms.
- **Offline.** No network; schemas come from `--did` files (fetch them with
  `icp canister metadata <id> candid:service`).

See `tests/integration_test.rs` for the full behavioural spec.
