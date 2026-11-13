-- Streem sync schema: the five tables each device keeps a local copy of.
--
-- Every row belongs to one account (auth.users). A signed-in account can read
-- and write only its own rows. The anonymous role gets no access at all.
--
-- updated_at is milliseconds since 1970, set by the device that made the
-- change. The row guard keeps the newest change: an update carrying an older
-- updated_at than the stored row is ignored.
--
-- Safe to run more than once.

-- Keeps the newest change and stamps the server time.

create or replace function public.streem_row_guard() returns trigger language plpgsql as $$
begin
  if tg_op = 'UPDATE' and new.updated_at < old.updated_at then
    return old;            -- a stale device must not overwrite a newer change
  end if;
  new.server_updated_at := now();
  return new;
end $$;

-- profiles: the profiles of an account. Soft-deleted with `deleted`.
-- real_debrid_key, audio_lang and subtitle_lang are the profile's overrides;
-- null means the account's value is used.

create table if not exists public.profiles (
  id uuid primary key,
  account_id uuid not null references auth.users(id) on delete cascade,
  name text not null,
  real_debrid_key text,
  audio_lang text,
  subtitle_lang text,
  deleted boolean not null default false,
  updated_at bigint not null,
  server_updated_at timestamptz not null default now()
);

create index if not exists profiles_account_id_server_updated_at_idx
  on public.profiles (account_id, server_updated_at);

alter table public.profiles enable row level security;
revoke all on public.profiles from anon;
grant select, insert, update, delete on public.profiles to authenticated;
drop policy if exists profiles_owner on public.profiles;
create policy profiles_owner on public.profiles
  for all to authenticated
  using (account_id = auth.uid())
  with check (account_id = auth.uid());

drop trigger if exists profiles_row_guard on public.profiles;
create trigger profiles_row_guard
  before insert or update on public.profiles
  for each row execute function public.streem_row_guard();

-- account_settings: the account's own settings, one row per account.

create table if not exists public.account_settings (
  account_id uuid primary key references auth.users(id) on delete cascade,
  real_debrid_key text,
  audio_lang text,
  subtitle_lang text,
  updated_at bigint not null,
  server_updated_at timestamptz not null default now()
);

create index if not exists account_settings_account_id_server_updated_at_idx
  on public.account_settings (account_id, server_updated_at);

alter table public.account_settings enable row level security;
revoke all on public.account_settings from anon;
grant select, insert, update, delete on public.account_settings to authenticated;
drop policy if exists account_settings_owner on public.account_settings;
create policy account_settings_owner on public.account_settings
  for all to authenticated
  using (account_id = auth.uid())
  with check (account_id = auth.uid());

drop trigger if exists account_settings_row_guard on public.account_settings;
create trigger account_settings_row_guard
  before insert or update on public.account_settings
  for each row execute function public.streem_row_guard();

-- addons: the account's addon list, shared by every profile.
-- The url may contain the {real_debrid_key} placeholder.

create table if not exists public.addons (
  account_id uuid not null references auth.users(id) on delete cascade,
  url text,
  position integer not null default 0,
  manifest_json text not null,
  deleted boolean not null default false,
  updated_at bigint not null,
  server_updated_at timestamptz not null default now(),
  primary key (account_id, url)
);

create index if not exists addons_account_id_server_updated_at_idx
  on public.addons (account_id, server_updated_at);

alter table public.addons enable row level security;
revoke all on public.addons from anon;
grant select, insert, update, delete on public.addons to authenticated;
drop policy if exists addons_owner on public.addons;
create policy addons_owner on public.addons
  for all to authenticated
  using (account_id = auth.uid())
  with check (account_id = auth.uid());

drop trigger if exists addons_row_guard on public.addons;
create trigger addons_row_guard
  before insert or update on public.addons
  for each row execute function public.streem_row_guard();

-- watch_progress: a profile's watch history and continue-watching positions.
-- No foreign key from profile_id to profiles: rows are soft-deleted and may
-- arrive in any order.

create table if not exists public.watch_progress (
  account_id uuid not null references auth.users(id) on delete cascade,
  profile_id uuid not null,
  video_id text,
  meta_id text not null,
  meta_type text not null,
  name text not null,
  poster text,
  position_secs double precision not null,
  duration_secs double precision not null,
  deleted boolean not null default false,
  updated_at bigint not null,
  server_updated_at timestamptz not null default now(),
  primary key (profile_id, video_id)
);

create index if not exists watch_progress_account_id_server_updated_at_idx
  on public.watch_progress (account_id, server_updated_at);

alter table public.watch_progress enable row level security;
revoke all on public.watch_progress from anon;
grant select, insert, update, delete on public.watch_progress to authenticated;
drop policy if exists watch_progress_owner on public.watch_progress;
create policy watch_progress_owner on public.watch_progress
  for all to authenticated
  using (account_id = auth.uid())
  with check (account_id = auth.uid());

drop trigger if exists watch_progress_row_guard on public.watch_progress;
create trigger watch_progress_row_guard
  before insert or update on public.watch_progress
  for each row execute function public.streem_row_guard();

-- library_items: a profile's library. No foreign key from profile_id to
-- profiles, for the same reason as watch_progress.

create table if not exists public.library_items (
  account_id uuid not null references auth.users(id) on delete cascade,
  profile_id uuid not null,
  meta_id text,
  meta_type text not null,
  name text not null,
  poster text,
  deleted boolean not null default false,
  updated_at bigint not null,
  server_updated_at timestamptz not null default now(),
  primary key (profile_id, meta_id)
);

create index if not exists library_items_account_id_server_updated_at_idx
  on public.library_items (account_id, server_updated_at);

alter table public.library_items enable row level security;
revoke all on public.library_items from anon;
grant select, insert, update, delete on public.library_items to authenticated;
drop policy if exists library_items_owner on public.library_items;
create policy library_items_owner on public.library_items
  for all to authenticated
  using (account_id = auth.uid())
  with check (account_id = auth.uid());

drop trigger if exists library_items_row_guard on public.library_items;
create trigger library_items_row_guard
  before insert or update on public.library_items
  for each row execute function public.streem_row_guard();
