#!/usr/bin/env bash
# Proves on a live Supabase project that the access rules from
# migrations/0001_init.sql hold: an account sees and writes only its own rows,
# the anonymous key sees nothing, and a stale change does not overwrite a newer one.
#
# Needs only bash, curl, grep and sed. Reads STREEM_SUPABASE_URL and
# STREEM_SUPABASE_KEY. Exits non-zero on the first failed check.
#
# Signs up two throwaway accounts; delete them afterwards in the dashboard
# under Authentication -> Users.

set -u

if [ -z "${STREEM_SUPABASE_URL-}" ] || [ -z "${STREEM_SUPABASE_KEY-}" ]; then
  echo "FAIL: set STREEM_SUPABASE_URL and STREEM_SUPABASE_KEY" >&2
  exit 1
fi
URL=${STREEM_SUPABASE_URL%/}
KEY=$STREEM_SUPABASE_KEY

# Some projects refuse test domains. If sign-up says the email is invalid,
# set STREEM_CHECK_EMAIL_DOMAIN to a domain the project accepts.
EMAIL_DOMAIN=${STREEM_CHECK_EMAIL_DOMAIN:-example.com}

STATUS=""
BODY=""

fail() {
  echo "FAIL: $1" >&2
  exit 1
}

# Like fail, but also prints the last response.
fail_http() {
  fail "$1
  HTTP $STATUS: $BODY"
}

# request METHOD PATH [TOKEN] [JSON] [EXTRA_HEADER]
# Sets STATUS and BODY. With no TOKEN the request carries only the apikey.
request() {
  local method=$1 path=$2 token=${3-} data=${4-} extra=${5-}
  local args=(-sS -X "$method" -H "apikey: $KEY" -H "Content-Type: application/json" -w '\n%{http_code}')
  if [ -n "$token" ]; then args+=(-H "Authorization: Bearer $token"); fi
  if [ -n "$data" ]; then args+=(--data "$data"); fi
  if [ -n "$extra" ]; then args+=(-H "$extra"); fi
  local out
  out=$(curl "${args[@]}" "$URL$path") || fail "could not reach $URL$path"
  STATUS=${out##*$'\n'}
  BODY=${out%$'\n'*}
}

# A random version 4 uuid, for profile ids (profiles.id has no default).
new_uuid() {
  printf '%04x%04x-%04x-4%03x-%04x-%04x%04x%04x' \
    "$RANDOM" "$RANDOM" "$RANDOM" $((RANDOM & 0xfff)) $(((RANDOM & 0x3fff) | 0x8000)) \
    "$RANDOM" "$RANDOM" "$RANDOM"
}

UUID_RE='^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'

# new_account LABEL: signs up a throwaway account, sets TOKEN and ACCOUNT_ID.
new_account() {
  local email="streem-check-$RANDOM$RANDOM$RANDOM@$EMAIL_DOMAIN"
  local password="Streem-check-$RANDOM$RANDOM$RANDOM"
  request POST /auth/v1/signup "" "{\"email\":\"$email\",\"password\":\"$password\"}"
  case $STATUS in
    2??) ;;
    *) fail_http "sign up account $1" ;;
  esac
  TOKEN=$(printf '%s' "$BODY" | sed -n 's/.*"access_token":"\([^"]*\)".*/\1/p')
  if [ -z "$TOKEN" ]; then
    fail "sign up account $1 returned no access_token; turn off \"Confirm email\" (see supabase/README.md)"
  fi
  ACCOUNT_ID=$(printf '%s' "$BODY" | sed -n 's/.*"user":{"id":"\([^"]*\)".*/\1/p')
  if ! [[ $ACCOUNT_ID =~ $UUID_RE ]]; then
    fail "sign up account $1: could not read the account id from the response"
  fi
}

# 1. Two throwaway accounts.
new_account A
A_TOKEN=$TOKEN
A_ACCOUNT_ID=$ACCOUNT_ID
new_account B
B_TOKEN=$TOKEN
echo "ok 1: signed up accounts A and B"

# 2. A inserts a profile.
PROFILE_ID=$(new_uuid)
request POST /rest/v1/profiles "$A_TOKEN" \
  "{\"id\":\"$PROFILE_ID\",\"account_id\":\"$A_ACCOUNT_ID\",\"name\":\"Original\",\"updated_at\":2000}"
[ "$STATUS" = 201 ] || fail_http "2: account A inserting its own profile, expected 201"
echo "ok 2: account A inserted a profile"

# 3. B sees none of A's rows.
request GET "/rest/v1/profiles?select=*" "$B_TOKEN"
[ "$STATUS" = 200 ] && [ "$BODY" = "[]" ] || fail_http "3: account B reading profiles, expected 200 and []"
echo "ok 3: account B sees no profiles"

# 4. B cannot write a row owned by A (fresh id, so no primary key conflict).
request POST /rest/v1/profiles "$B_TOKEN" \
  "{\"id\":\"$(new_uuid)\",\"account_id\":\"$A_ACCOUNT_ID\",\"name\":\"Intruder\",\"updated_at\":2000}"
case $STATUS in
  401|403) ;;
  *) fail_http "4: account B inserting a profile for account A, expected 401 or 403" ;;
esac
echo "ok 4: account B cannot insert a profile for account A"

# 5. The anonymous key alone sees no rows: either an empty list, or refused outright.
request GET "/rest/v1/profiles?select=*"
case $STATUS in
  200) [ "$BODY" = "[]" ] || fail_http "5: reading profiles without signing in, expected no rows" ;;
  401|403) ;;
  *) fail_http "5: reading profiles without signing in, expected 200 [] or 401/403" ;;
esac
echo "ok 5: no rows without signing in"

# 6. A stale change (older updated_at) does not overwrite the newer one.
# return=representation proves the update reached the row rather than matching nothing.
request PATCH "/rest/v1/profiles?id=eq.$PROFILE_ID" "$A_TOKEN" \
  '{"name":"Stale","updated_at":1000}' "Prefer: return=representation"
[ "$STATUS" = 200 ] || fail_http "6: account A sending a stale update, expected 200"
printf '%s' "$BODY" | grep -q "\"id\":\"$PROFILE_ID\"" \
  || fail_http "6: the stale update did not reach account A's profile"
request GET "/rest/v1/profiles?id=eq.$PROFILE_ID&select=name" "$A_TOKEN"
[ "$STATUS" = 200 ] && [ "$BODY" = '[{"name":"Original"}]' ] \
  || fail_http "6: reading the profile back, expected the original name"
echo "ok 6: a stale update kept the newer name"

echo "ALL CHECKS PASSED"
