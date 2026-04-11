#!/usr/bin/env bash
set -euo pipefail

BASE_URL="${BASE_URL:-http://127.0.0.1:8080}"
APPLE_USERNAME="${APPLE_USERNAME:-}"
APPLE_PASSWORD="${APPLE_PASSWORD:-}"
APPLE_2FA="${APPLE_2FA:-}"
SEARCH_KEYWORD="${SEARCH_KEYWORD:-Taylor Swift}"
DOWNLOAD_CODEC="${DOWNLOAD_CODEC:-alac}"

if [ -z "$APPLE_USERNAME" ] || [ -z "$APPLE_PASSWORD" ]; then
	echo "ERROR: APPLE_USERNAME and APPLE_PASSWORD are required."
	echo "Example: APPLE_USERNAME='apple@example.com' APPLE_PASSWORD='secret' $0"
	exit 1
fi

require_cmd() {
	if ! command -v "$1" >/dev/null 2>&1; then
		echo "ERROR: missing command '$1'"
		exit 1
	fi
}

require_cmd curl
require_cmd jq

json_post() {
	local endpoint="$1"
	local body="$2"
	local outfile="$3"
	curl -sS -o "$outfile" -w "%{http_code}" \
		-X POST "$BASE_URL$endpoint" \
		-H "content-type: application/json" \
		-d "$body"
}

json_get() {
	local endpoint="$1"
	local outfile="$2"
	curl -sS -o "$outfile" -w "%{http_code}" "$BASE_URL$endpoint"
}

assert_json_field_eq() {
	local file="$1"
	local path="$2"
	local expected="$3"
	local actual
	actual="$(jq -r "$path // empty" "$file")"
	if [ "$actual" != "$expected" ]; then
		echo "ERROR: expected $path=$expected, got '$actual'"
		echo "Response:"
		cat "$file"
		exit 1
	fi
}

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

echo "==> reset stale auth state"
curl -sS -X POST "$BASE_URL/login/reset" -o /dev/null || true
curl -sS -X POST "$BASE_URL/logout" -o /dev/null || true

echo "==> login"
login_json="$tmpdir/login.json"
login_status="$(json_post "/login" "{\"username\":\"$APPLE_USERNAME\",\"password\":\"$APPLE_PASSWORD\"}" "$login_json")"
if [ "$login_status" != "200" ]; then
	echo "ERROR: /login failed with HTTP $login_status"
	cat "$login_json"
	exit 1
fi

login_state="$(jq -r '.state // empty' "$login_json")"
login_status_field="$(jq -r '.status // empty' "$login_json")"
if [ "$login_status_field" = "need_2fa" ]; then
	if [ -z "$APPLE_2FA" ]; then
		read -r -p "2FA code required, input APPLE_2FA code: " APPLE_2FA
	fi
	if [ -z "$APPLE_2FA" ]; then
		echo "ERROR: 2FA code is empty"
		exit 1
	fi

	echo "==> submit 2FA"
	twofa_json="$tmpdir/twofa.json"
	twofa_status="$(json_post "/login/2fa" "{\"code\":\"$APPLE_2FA\"}" "$twofa_json")"
	if [ "$twofa_status" != "200" ]; then
		echo "ERROR: /login/2fa failed with HTTP $twofa_status"
		cat "$twofa_json"
		exit 1
	fi
	assert_json_field_eq "$twofa_json" '.status' 'ok'
	assert_json_field_eq "$twofa_json" '.state' 'logged_in'
else
	if [ "$login_state" != "logged_in" ]; then
		echo "ERROR: login did not reach logged_in state"
		cat "$login_json"
		exit 1
	fi
fi

echo "==> check status"
status_json="$tmpdir/status.json"
status_code="$(json_get "/status" "$status_json")"
if [ "$status_code" != "200" ]; then
	echo "ERROR: /status failed with HTTP $status_code"
	cat "$status_json"
	exit 1
fi
assert_json_field_eq "$status_json" '.state' 'logged_in'

search_type_and_pick_song() {
	local search_type="$1"
	local data_key="$2"
	local out="$tmpdir/search_${search_type}.json"

	echo "==> search ${search_type}: $SEARCH_KEYWORD"
	local code
	code="$(json_get "/search?query=$(jq -rn --arg q "$SEARCH_KEYWORD" '$q|@uri')&type=${search_type}&limit=5" "$out")"
	if [ "$code" != "200" ]; then
		echo "ERROR: /search type=${search_type} failed with HTTP $code"
		cat "$out"
		exit 1
	fi

	local count
	count="$(jq -r ".results.${data_key}.data | length" "$out")"
	if [ "$count" = "0" ] || [ "$count" = "null" ]; then
		echo "ERROR: no ${search_type} results for '$SEARCH_KEYWORD'"
		cat "$out"
		exit 1
	fi

	local first_id first_name
	first_id="$(jq -r ".results.${data_key}.data[0].id" "$out")"
	first_name="$(jq -r ".results.${data_key}.data[0].attributes.name // .results.${data_key}.data[0].attributes.artistName // \"(unknown)\"" "$out")"
	echo "    first ${search_type}: id=${first_id}, name=${first_name}"
}

search_type_and_pick_song "song" "songs"
search_type_and_pick_song "album" "albums"
search_type_and_pick_song "artist" "artists"

song_id="$(jq -r '.results.songs.data[0].id' "$tmpdir/search_song.json")"
if [ -z "$song_id" ] || [ "$song_id" = "null" ]; then
	echo "ERROR: could not pick song id from song search"
	exit 1
fi

echo "==> download playback for song_id=$song_id"
playback_json="$tmpdir/playback.json"
playback_code="$(json_get "/playback/${song_id}?codec=${DOWNLOAD_CODEC}" "$playback_json")"
if [ "$playback_code" != "200" ]; then
	echo "ERROR: /playback/$song_id failed with HTTP $playback_code"
	cat "$playback_json"
	exit 1
fi

playback_path="$(jq -r '.playbackUrl // empty' "$playback_json")"
playback_size="$(jq -r '.size // 0' "$playback_json")"

if [ -z "$playback_path" ] || [ "$playback_size" = "0" ]; then
	echo "ERROR: invalid playback response"
	cat "$playback_json"
	exit 1
fi

echo "    playback path: $playback_path"
echo "    playback size: $playback_size"

if ! curl -sS -f -r 0-0 -o /dev/null "$BASE_URL/$playback_path"; then
	echo "ERROR: cached playback file is not reachable: $BASE_URL/$playback_path"
	exit 1
fi

echo "==> logout"
logout_json="$tmpdir/logout.json"
logout_code="$(json_post "/logout" "{}" "$logout_json")"
if [ "$logout_code" != "200" ]; then
	echo "ERROR: /logout failed with HTTP $logout_code"
	cat "$logout_json"
	exit 1
fi

echo "PASS: login -> search(song/album/artist) -> download playback"
