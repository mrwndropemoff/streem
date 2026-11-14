# 14: Create the Supabase project and apply the schema

**What to build:** A live Supabase project on the free tier with the schema from ticket 13 applied and verified, and its address and public key recorded where the build reads them.

**Blocked by:** 13

**Status:** ready-for-human

## Why a human

It needs a Supabase login and choices made in its dashboard.

## Read first

- `docs/adr/0001-hosted-supabase-with-local-copy.md`
- `supabase/README.md`

## Steps

1. Create a project at supabase.com on the free plan. Pick the region closest to where the app will be used.
2. In Authentication settings, under the Email provider, turn **off** "Confirm email". Sign-up in the app then signs the person in straight away. Everyone on an **account** is trusted, so nothing is lost.
3. Open the SQL editor, paste the whole of `supabase/migrations/0001_init.sql`, and run it.
4. From the project's API settings, copy the Project URL and the public key (labelled "anon" or "publishable"). Never copy the "service role" or "secret" key into this repository.
5. Create `.cargo/config.toml` at the repo root:

   ```toml
   [env]
   STREEM_SUPABASE_URL = "https://<project>.supabase.co"
   STREEM_SUPABASE_KEY = "<public key>"
   ```

   The public key is designed to be shipped inside apps; the access rules from ticket 13 are what protect the data. It is fine to commit this file while the repository is private. Before open-sourcing, move these values out of the repository so strangers do not use this backend.
6. Run the check:

   ```
   STREEM_SUPABASE_URL=... STREEM_SUPABASE_KEY=... ./supabase/check-rls.sh
   ```

7. Delete the two throwaway accounts the script created, in the dashboard under Authentication → Users.

## Acceptance checks

- [ ] The five tables exist in the dashboard's table editor, each marked as having row level security enabled.
- [x] `./supabase/check-rls.sh` prints `ALL CHECKS PASSED`.
- [x] `.cargo/config.toml` exists with both values and contains no service-role key.
- [x] "Confirm email" is off.

## Good to know

A free project is paused after a week with no database activity. Data is kept; restore it from the dashboard.

## Comments

- 2026-11-14: Project `wgbouvhfqpdjsgslwsah` created and the migration applied in the dashboard. `.cargo/config.toml` holds the Project URL and the publishable key (`sb_publishable_…`), and no secret or service-role key. `./supabase/check-rls.sh` ran against the live project and printed checks 1–6 `ok` and `ALL CHECKS PASSED`. That also shows "Confirm email" is off, because sign-up returned an access token straight away.
- Not checked from here: the table editor's row level security badge on all five tables (the script only uses `profiles`). The two `streem-check-…@example.com` accounts still need deleting under Authentication → Users.
- 2026-11-14: The repository is public, so `.cargo/config.toml` is gitignored instead of committed (step 5's "fine while private" doesn't apply). `.cargo/config.example.toml` is the template, and `supabase/README.md` says to copy it. The real file exists only in this checkout.
