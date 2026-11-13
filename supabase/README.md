# Supabase

The sync schema and access rules for the hosted project (see `docs/adr/0001-hosted-supabase-with-local-copy.md`).

**Apply the migration:** open the project's SQL editor, paste the whole of `migrations/0001_init.sql` and run it. Running it again is safe.

**Check the access rules:** with "Confirm email" turned off, run
`STREEM_SUPABASE_URL=https://<project>.supabase.co STREEM_SUPABASE_KEY=<public key> ./supabase/check-rls.sh`
from the repo root. It prints `ALL CHECKS PASSED`, or the first check that failed.
It signs up two throwaway accounts: delete them afterwards under Authentication → Users.
