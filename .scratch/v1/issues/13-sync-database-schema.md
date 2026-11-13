# 13: Sync database schema and access rules

**What to build:** The SQL that creates the five synced tables on Supabase (Postgres), with rules so that a signed-in **account** can read and write only its own rows, plus a script that proves those rules hold. Nothing is run against a live server in this ticket; a human applies it in ticket 14.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

## Read first

- `CONTEXT.md` (**Account**, **Profile**, **Override**)
- `docs/adr/0001-hosted-supabase-with-local-copy.md`
- `.scratch/v1/spec.md`, sections "Account, profiles and overrides" and "Sync".

## Files

- New: `supabase/migrations/0001_init.sql`
- New: `supabase/check-rls.sh`
- New: `supabase/README.md` (ten lines at most: how to apply the migration in the Supabase SQL editor and how to run the check script)

## Tables

All in the `public` schema. `account_id` is always `uuid not null references auth.users(id) on delete cascade`. `updated_at` is `bigint not null`: milliseconds since 1970, set by the device that made the change. `server_updated_at` is `timestamptz not null default now()`.

| Table | Columns besides `account_id`, `updated_at`, `server_updated_at` | Primary key |
|---|---|---|
| `profiles` | `id uuid`, `name text not null`, `real_debrid_key text`, `audio_lang text`, `subtitle_lang text`, `deleted boolean not null default false` | `id` |
| `account_settings` | `real_debrid_key text`, `audio_lang text`, `subtitle_lang text` | `account_id` |
| `addons` | `url text`, `position integer not null default 0`, `manifest_json text not null`, `deleted boolean not null default false` | `(account_id, url)` |
| `watch_progress` | `profile_id uuid not null`, `video_id text`, `meta_id text not null`, `meta_type text not null`, `name text not null`, `poster text`, `position_secs double precision not null`, `duration_secs double precision not null`, `deleted boolean not null default false` | `(profile_id, video_id)` |
| `library_items` | `profile_id uuid not null`, `meta_id text`, `meta_type text not null`, `name text not null`, `poster text`, `deleted boolean not null default false` | `(profile_id, meta_id)` |

No foreign key from `profile_id` to `profiles`: rows are soft-deleted and may arrive in any order.

Add an index on `(account_id, server_updated_at)` for every table.

## Access rules

For every table:

```sql
alter table public.<t> enable row level security;
grant select, insert, update, delete on public.<t> to authenticated;
create policy <t>_owner on public.<t>
  for all to authenticated
  using (account_id = auth.uid())
  with check (account_id = auth.uid());
```

Grant nothing to the `anon` role.

## Keeping the newest change

One trigger function, attached `before insert or update` on every table:

```sql
create function public.streem_row_guard() returns trigger language plpgsql as $$
begin
  if tg_op = 'UPDATE' and new.updated_at < old.updated_at then
    return old;            -- a stale device must not overwrite a newer change
  end if;
  new.server_updated_at := now();
  return new;
end $$;
```

## The check script (`supabase/check-rls.sh`)

Uses only `bash`, `curl`, `grep` and `sed`. Reads `STREEM_SUPABASE_URL` and `STREEM_SUPABASE_KEY` from the environment and exits non-zero on the first failed check, printing which one.

1. Sign up two throwaway accounts A and B (`POST $URL/auth/v1/signup`, header `apikey: $KEY`, JSON body with a random `email` and a `password`). Take each `access_token` and user id from the response.
2. As A, insert a profile row (`POST $URL/rest/v1/profiles`, headers `apikey: $KEY` and `Authorization: Bearer <A token>`). Expect HTTP 201.
3. As B, `GET $URL/rest/v1/profiles?select=*`. Expect the body `[]`.
4. As B, insert a profile row with A's `account_id`. Expect HTTP 401 or 403.
5. With no `Authorization` header, `GET $URL/rest/v1/profiles?select=*`. Expect no rows.
6. As A, update the row with an older `updated_at` and a different name, then read it back. Expect the original name.
7. Print `ALL CHECKS PASSED`.

## Out of scope

Running anything against a live project, Realtime configuration, any Rust code.

## Acceptance checks

- [x] `supabase/migrations/0001_init.sql` creates the five tables with exactly the columns above.
- [x] `grep -c "enable row level security" supabase/migrations/0001_init.sql` prints `5`.
- [x] `grep -c "create policy" supabase/migrations/0001_init.sql` prints `5`.
- [x] `grep -ci "to anon" supabase/migrations/0001_init.sql` prints `0`.
- [x] The trigger is created on all five tables.
- [x] `bash -n supabase/check-rls.sh` reports no syntax error, and the script is executable.
- [ ] The file can be run twice without error (use `if not exists` and `create or replace` where Postgres allows; drop and recreate policies and triggers).

## Rules

- Use the terms defined in `CONTEXT.md` in names and comments.
- Do not install anything. If something is missing, stop and write what is missing under `## Comments` in this file.
- Do only what this ticket lists.
- When done, tick the boxes and add a line under `## Comments` saying what you verified and how.

## Comments

- Verified 2026-11-13 by running the acceptance greps (`enable row level security` 5, `create policy` 5, `to anon` 0, plus `create trigger … streem_row_guard` 5, `create table if not exists` 5 and the `(account_id, server_updated_at)` index 5), `bash -n supabase/check-rls.sh` (no error), and `git ls-files -s` (the script is `100755`). I checked the columns against the table above by reading the file.
- Missing: there is no Postgres here (`psql` is not installed and the Docker daemon is not running), so the SQL has **not been executed**. The "run twice" box stays unticked. The file is written for it (`create … if not exists`, `create or replace function`, and `drop … if exists` before every policy and trigger), but ticket 14 is the first real run.
- `check-rls.sh` has not been run against Supabase. Its logic was run against a fake `curl` that mimics Supabase's responses: it prints `ALL CHECKS PASSED` when the rules hold, and stops with the HTTP status and body when B can see A's rows, when the stale PATCH matches no row, when the stale PATCH overwrites the name, or when sign-up returns no token because "Confirm email" is on.
- Small additions:
  - Each table gets `revoke all … from anon`, because Supabase's default privileges grant every new `public` table to that role. With this, "grant nothing" is true, not just hidden by row level security. Step 5 therefore accepts either `200 []` or `401`/`403`.
  - Step 6's PATCH sends `Prefer: return=representation` and must return the row, so an update that matched nothing cannot pass.
  - An optional `STREEM_CHECK_EMAIL_DOMAIN` (default `example.com`) is there in case the project refuses that domain at sign-up.
  - A root `.gitattributes` keeps `*.sh` files on LF. With `core.autocrlf=true`, a Windows checkout would otherwise write CRLF and bash would fail.
