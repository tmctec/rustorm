#!/bin/bash
# Regenerates the Reading output example fixtures (docs/cli.md) with the
# rustorm binary itself, so the fixtures are exactly what the tool writes:
#   crates/rustorm/tests/fixtures/read.conf           sectioned hosts with metadata
#   crates/rustorm/tests/fixtures/read-root.conf      a root that includes config.d/*
#   crates/rustorm/tests/fixtures/read-df-austin.conf the included file holding D72
# Run from the repository root after `make build`.
set -eu
B=${RUSTORM_BIN:-target/debug/rustorm}
F=crates/rustorm/tests/fixtures
run() { "$B" --config "$1" --no-backup "${@:2}" >/dev/null; }

: > "$F/read.conf"
run "$F/read.conf" add D72 travis@10.7.112.72 --section "df austin"
run "$F/read.conf" add D73 travis@10.7.112.73 --section "df austin"
run "$F/read.conf" add buildbox deploy@10.0.4.12 --section other
run "$F/read.conf" add github git@github.com --section other
run "$F/read.conf" add cache1 travis@10.7.112.80 --section other
run "$F/read.conf" set D72 --tag db
run "$F/read.conf" set cache1 --tag cache
run "$F/read.conf" set buildbox note "Primary build box" location "Austin DC, rack 4, U12" privateKeyLocation keepassxc other "owner alice" tags "prod, austin, db"
run "$F/read.conf" set -a buildbox note "Reboot only after 18:00"
# D72 keeps lower-case spellings so the examples show txt output keeping the file's own.
perl -0pi -e 's/Host D72\n    HostName 10\.7\.112\.72\n    User travis/Host D72\n    hostname 10.7.112.72\n    user travis/' "$F/read.conf"

: > "$F/read-root.conf"
run "$F/read-root.conf" add github git@github.com --section other
{ printf 'Include config.d/*\n\n'; cat "$F/read-root.conf"; } > "$F/read-root.tmp"
mv "$F/read-root.tmp" "$F/read-root.conf"

: > "$F/read-df-austin.conf"
run "$F/read-df-austin.conf" add D72 travis@10.7.112.72 --section "df austin"
echo "fixtures written: read.conf read-root.conf read-df-austin.conf"
