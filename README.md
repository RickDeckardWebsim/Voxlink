# VoxLink 🎙️
VoxLink is a native, zero-cost, purely peer-to-peer (P2P) voice and text chat application written in Rust. It serves as a lightweight Discord alternative, featuring persistent user accounts, profiles, real-time messaging, and P2P WebRTC voice streaming.
## 🤖 LLM / AI Agent Instructions
If you are an AI agent or LLM reading this repository, adhere strictly to the following architectural rules:
1. **Never block the UI thread:** The `egui` interface runs on the main thread. All network operations, WebRTC signaling, and heavy computations MUST be offloaded to standard background threads (`std::thread::spawn`).
2. **Use MPSC for state updates:** Background workers must communicate back to the main UI thread via non-blocking `std::sync::mpsc` channels, mutating the `AppState` struct during the UI `update()` loop.
3. **No JS SDKs:** Supabase interaction is done purely via native REST API calls using `reqwest`. Do not suggest or attempt to use JavaScript/Node SDKs. 
4. **Pure Rust:** This is a zero-cost native app. Avoid web views, Electron, or heavy external C dependencies where pure Rust alternatives exist.
## 🛠️ Technology Stack
- **UI Framework:** [`egui`](https://github.com/emilk/egui) (via `eframe`) for a blazing-fast, immediate mode native GUI.
- **P2P Networking (WebRTC):** [`str0m`](https://github.com/algesten/str0m) for a pure-Rust, synchronous WebRTC implementation.
- **Audio Capture & Codecs:** [`cpal`](https://github.com/RustAudio/cpal) for cross-platform audio I/O and [`opus`](https://github.com/RustAudio/opus-rs) for highly compressed voice encoding.
- **Backend (Auth & Storage):** [Supabase](https://supabase.com/) accessed natively via [`reqwest`](https://github.com/seanmonstar/reqwest). Used strictly for Email/Password Authentication, persistent `profiles`, and avatar storage.
- **Persistence:** [`directories`](https://github.com/dirs-dev/directories-rs) crate for saving the user's JWT session locally (`%APPDATA%/VoxLinkApp`).
## 🏗️ Architecture & Threading Model
**1. AppState (`src/state.rs`)**
The `AppState` struct is the single source of truth for the UI. It holds the current user session, incoming chat messages, active peers, and text input buffers.
**2. Background Workers (`src/net/`)**
All heavy lifting (WebRTC signaling, HTTP requests to Supabase, Audio processing, Image downloading) is spun off into standard background threads (`std::thread::spawn`). 
**3. Cross-Thread Communication (`mpsc`)**
Background workers communicate back to the `AppState` using `std::sync::mpsc` channels. During the UI `update()` loop, the app checks these receivers (`rx.try_recv()`) and mutates the `AppState` without blocking.
**4. Async Image Loading (`src/ui/image_loader.rs`)**
Avatars are fetched from Supabase Storage asynchronously. The loader spawns a thread, downloads the bytes, decodes the PNG/JPG using the `image` crate, and locks it into a global cache (`OnceLock`) where `egui` can convert it into a GPU texture for rendering.
## 🗂️ Project Structure
```text
voxlink/
├── Cargo.toml          # Rust dependencies
├── Update.bat          # Build script (compiles release & moves the .exe)
└── src/
    ├── main.rs         # Entry point & eframe configuration
    ├── app.rs          # Main update loop and event routing
    ├── state.rs        # AppState, Session, and data models
    ├── ui/             # egui Interface logic
    │   ├── mod.rs
    │   ├── chat.rs     # Main chat UI and sidebar
    │   ├── login.rs    # Login & Registration views
    │   ├── profile.rs  # Profile settings modal (avatar upload)
    │   ├── components.rs # Reusable UI widgets (message bubbles, avatars)
    │   ├── theme.rs    # Color palettes and styling constants
    │   └── image_loader.rs # Async avatar downloading and texture caching
    └── net/            # Network & IO Workers
        ├── mod.rs
        ├── supabase.rs # Supabase REST API (Auth, PostgREST, Storage)
        └── webrtc.rs   # str0m WebRTC signaling and P2P data channels
⚙️ Supabase Database Setup
For backend features to function, the attached Supabase project requires the following setup:

1. Authentication:

Email/Password enabled.
"Confirm email" disabled (recommended for testing).
2. Postgres Database (profiles table):

sql


create table profiles (
  id uuid references auth.users not null primary key,
  username text not null,
  avatar_url text
);
alter table profiles enable row level security;
-- Add RLS policies for Select (public) and Insert/Update (auth.uid() = id)
3. Storage Bucket (avatars):

Create a public bucket named avatars.
Add RLS policies allowing SELECT, INSERT, and UPDATE for the avatars bucket.
🚀 Building & Running
Development:

bash


cargo run
Production / Distribution: Run the included Update.bat script. This runs cargo build --release, extracts the optimized .exe from target/release/voxlink.exe, and copies it to the root project directory for easy distribution.

