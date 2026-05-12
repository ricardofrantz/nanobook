#!/usr/bin/env sh
set -eu

base_url="https://emi.nasdaq.com/ITCH/Nasdaq%20ITCH"
file_name="07302019.NASDAQ_ITCH50.gz"
data_dir="$(CDPATH= cd -- "$(dirname -- "$0")/data" && pwd)"
expected_md5="$(CDPATH= cd -- "$(dirname -- "$0")/expected" && pwd)/sample.md5"
output="$data_dir/$file_name"
url="$base_url/$file_name"

mkdir -p "$data_dir"

checksum_tool() {
  if command -v md5sum >/dev/null 2>&1; then
    printf '%s\n' md5sum
  elif command -v md5 >/dev/null 2>&1; then
    printf '%s\n' md5
  else
    printf '%s\n' "missing"
  fi
}

actual_md5() {
  case "$(checksum_tool)" in
    md5sum) md5sum "$1" | awk '{print $1}' ;;
    md5) md5 -q "$1" ;;
    *) echo "md5sum or md5 is required" >&2; exit 1 ;;
  esac
}

expected="$(awk '{print $1}' "$expected_md5")"

if [ -f "$output" ]; then
  actual="$(actual_md5 "$output")"
  if [ "$actual" = "$expected" ]; then
    echo "$output already exists and matches $expected"
    exit 0
  fi
  echo "$output exists but checksum is $actual, expected $expected; re-downloading" >&2
  rm -f "$output"
fi

tmp="$output.part"
curl -fL --continue-at - --output "$tmp" "$url"
actual="$(actual_md5 "$tmp")"

if [ "$actual" != "$expected" ]; then
  echo "checksum mismatch for $tmp: got $actual, expected $expected" >&2
  exit 1
fi

mv "$tmp" "$output"
echo "downloaded $output"
