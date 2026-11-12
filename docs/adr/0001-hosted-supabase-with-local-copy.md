# Hosted Supabase for sync, with a local copy on each device

An account and its profiles must follow people across devices, and a watch party needs devices to exchange play, pause and seek signals in real time. We use Supabase's hosted free tier for login, data and those signals, and each device keeps a local copy of the account's data that the UI reads; sync runs in the background and the most recent change wins. One service covers all three needs with no server of ours to run, and reading locally keeps every screen from waiting on the network.

## Considered Options

- **Self-hosted Supabase**: rejected. It needs about 4 GB of RAM across 10 or more containers, more than free VMs offer, and it would make us responsible for updates, backups and certificates on a system holding everyone's Real-Debrid keys.
- **PocketBase on a small VM**: lighter, but still a server to run.
- **Our own server**: the most work for a group of about five people.
- **A database the app connects to directly**: rejected. The app would have to contain the database password, so anyone with the app could read every account.

## Consequences

- The free tier pauses a project after a week without database activity. Data is kept and the project is restored from the dashboard.
- Real-Debrid keys are stored on Supabase, protected by per-account access rules.
- Moving off Supabase later means replacing login, sync and watch-party signalling together.
