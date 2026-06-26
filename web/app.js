// ─────────────────────────────────────────────────────────────────────────────
// VoxLink Web Client — vanilla JS, no build step
// Talks to the exact same Supabase backend as the desktop app.
// ─────────────────────────────────────────────────────────────────────────────

import { createClient } from 'https://esm.sh/@supabase/supabase-js@2';

const SUPABASE_URL      = 'https://syftqwloslmnjyvppler.supabase.co';
const SUPABASE_ANON_KEY = 'sb_publishable_VK3kO0lX4tTsrHlCsH6JFQ_ebB6_lMH';
const CHANNEL_NAME      = 'p2p-signaling';
const SPEAKING_THRESHOLD  = 0.01;
const SILENCE_HOLD_FRAMES = 8;   // × 100 ms = 800 ms

const RTC_CONFIG = {
  iceServers: [
    { urls: 'stun:stun.l.google.com:19302' },
    { urls: 'stun:stun1.l.google.com:19302' },
  ],
};

// Avatar palette matches desktop theme.rs exactly
const AVATAR_PALETTE = [
  '#5865f2','#3ba55d','#eb459e','#f0b232',
  '#ed4245','#17a8e3','#9c59d1','#1abc9c',
];

// ── Supabase client ───────────────────────────────────────────────────────────
const sb = createClient(SUPABASE_URL, SUPABASE_ANON_KEY);

// ── State ─────────────────────────────────────────────────────────────────────
let myUserId, myUsername, myAvatarUrl, myDescription;
let sigChannel;
const peers       = {};
const peerConns   = {};
const respondedTo = new Set();

let localStream   = null;
let audioCtx      = null;
let speakingTimer = null;
let inVoice       = false;
let isMuted       = false;
let amSpeaking    = false;
let silenceFrames = 0;
let lastMsgAuthor = null;
let authMode      = 'signin';

// ── DOM shortcuts ─────────────────────────────────────────────────────────────
const $ = id => document.getElementById(id);

// ── Init ──────────────────────────────────────────────────────────────────────
async function init() {
  bindEvents();
  const { data: { session } } = await sb.auth.getSession();
  if (session) await enterChat(session);
  else showScreen('login-screen');
}

function bindEvents() {
  // Auth
  $('login-form').addEventListener('submit', handleLogin);
  $('auth-toggle-link').addEventListener('click', e => {
    e.preventDefault();
    authMode = authMode === 'signin' ? 'signup' : 'signin';
    const up = authMode === 'signup';
    $('username-row').style.display    = up ? '' : 'none';
    $('login-btn').textContent         = up ? 'Create Account' : 'Sign In';
    $('auth-toggle-text').textContent  = up ? 'Already have an account?' : "Don't have an account?";
    $('auth-toggle-link').textContent  = up ? 'Sign in' : 'Create one';
    $('login-error').textContent       = '';
    (up ? $('login-username') : $('login-email')).focus();
  });

  // Chat
  $('logout-btn').addEventListener('click', handleLogout);
  $('voice-toggle-btn').addEventListener('click', toggleVoice);
  $('mute-btn').addEventListener('click', toggleMute);
  $('send-btn').addEventListener('click', trySend);

  const input = $('message-input');
  const ph    = $('input-placeholder');
  input.addEventListener('input', () => { ph.style.display = input.textContent.trim() ? 'none' : ''; });
  input.addEventListener('keydown', e => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); trySend(); } });

  // Media attach
  $('attach-btn').addEventListener('click', () => $('file-input').click());
  $('file-input').addEventListener('change', e => {
    if (e.target.files[0]) uploadMedia(e.target.files[0]);
    e.target.value = '';
  });

  // Profile bar click → open modal
  $('profile-bar').addEventListener('click', openProfileModal);
  $('profile-bar').style.cursor = 'pointer';

  // Profile modal
  $('profile-close-btn').addEventListener('click', closeProfileModal);
  $('profile-cancel-btn').addEventListener('click', closeProfileModal);
  $('profile-save-btn').addEventListener('click', saveProfile);
  $('profile-modal').addEventListener('click', e => { if (e.target === $('profile-modal')) closeProfileModal(); });
  $('profile-avatar-preview').addEventListener('click', () => $('profile-avatar-input').click());
  $('profile-avatar-input').addEventListener('change', e => {
    if (e.target.files[0]) uploadProfileAvatar(e.target.files[0]);
    e.target.value = '';
  });
  $('profile-desc-input').addEventListener('input', () => {
    $('profile-desc-count').textContent = $('profile-desc-input').value.length;
  });
}

// ── Auth ──────────────────────────────────────────────────────────────────────
async function handleLogin(e) {
  e.preventDefault();
  const email    = $('login-email').value.trim();
  const password = $('login-password').value;
  $('login-error').textContent = '';
  setLoginBusy(true);

  if (authMode === 'signup') {
    const username = $('login-username').value.trim();
    if (!username) { $('login-error').textContent = 'Please enter a username.'; setLoginBusy(false); return; }

    const { data, error } = await sb.auth.signUp({ email, password });
    if (error) { $('login-error').textContent = error.message; setLoginBusy(false); return; }

    await sb.from('profiles').upsert({ id: data.user.id, username, avatar_url: null, description: '' });
    await enterChat(data.session);
  } else {
    const { data, error } = await sb.auth.signInWithPassword({ email, password });
    if (error) { $('login-error').textContent = error.message; setLoginBusy(false); return; }
    await enterChat(data.session);
  }
}

function setLoginBusy(busy) {
  $('login-btn').disabled    = busy;
  $('login-btn').textContent = busy ? 'Signing in…' : (authMode === 'signup' ? 'Create Account' : 'Sign In');
}

async function handleLogout() {
  await cleanup();
  await sb.auth.signOut();
  lastMsgAuthor = null;
  $('messages').innerHTML = '';
  showScreen('login-screen');
}

async function enterChat(session) {
  myUserId = session.user.id;
  await loadProfile();
  renderProfileBar();
  showScreen('chat-screen');
  connectSignaling();
}

async function loadProfile() {
  const { data } = await sb.from('profiles').select('*').eq('id', myUserId).single();
  if (data) {
    myUsername    = data.username    ?? 'User';
    myAvatarUrl   = data.avatar_url  ?? null;
    myDescription = data.description ?? '';
  }
}

async function cleanup() {
  if (inVoice) await leaveVoice();
  if (sigChannel) { sigChannel.unsubscribe(); sigChannel = null; }
  for (const pc of Object.values(peerConns)) pc.close();
  Object.keys(peerConns).forEach(k => delete peerConns[k]);
  Object.keys(peers).forEach(k => delete peers[k]);
  respondedTo.clear();
}

// ── Profile modal ─────────────────────────────────────────────────────────────
function openProfileModal() {
  $('profile-username-input').value  = myUsername    ?? '';
  $('profile-desc-input').value      = myDescription ?? '';
  $('profile-desc-count').textContent = ($('profile-desc-input').value.length);
  $('profile-modal-error').textContent = '';
  renderProfileModalAvatar();
  $('profile-modal').style.display = 'flex';
}

function closeProfileModal() {
  $('profile-modal').style.display = 'none';
}

function renderProfileModalAvatar() {
  const el = $('profile-avatar-preview');
  el.style.background = avatarColor(myUsername ?? 'User');
  el.innerHTML = myAvatarUrl
    ? `<img src="${esc(myAvatarUrl)}" alt="">`
    : esc((myUsername?.[0] ?? 'U').toUpperCase());
}

async function saveProfile() {
  const username    = $('profile-username-input').value.trim();
  const description = $('profile-desc-input').value;
  if (!username) { $('profile-modal-error').textContent = 'Username cannot be empty.'; return; }

  const btn = $('profile-save-btn');
  btn.disabled    = true;
  btn.textContent = 'Saving…';
  $('profile-modal-error').textContent = '';

  const { error } = await sb.from('profiles').update({ username, description }).eq('id', myUserId);
  if (error) {
    $('profile-modal-error').textContent = error.message;
    btn.disabled = false; btn.textContent = 'Save Changes';
    return;
  }

  const oldUsername = myUsername;
  myUsername    = username;
  myDescription = description;

  renderProfileBar();
  renderMembers();

  await bcast('profile_update', {
    from:         oldUsername,
    new_username: myUsername,
    avatar_url:   myAvatarUrl,
    description:  myDescription,
  });

  btn.disabled = false; btn.textContent = 'Save Changes';
  closeProfileModal();
}

async function uploadProfileAvatar(file) {
  const ext  = file.name.split('.').pop().toLowerCase() || 'png';
  const path = `${myUserId}_avatar.${ext}`;

  $('profile-save-btn').disabled    = true;
  $('profile-modal-error').textContent = '';

  const { error } = await sb.storage.from('avatars').upload(path, file, { contentType: file.type, upsert: true });
  if (error) {
    $('profile-modal-error').textContent = `Upload failed: ${error.message}`;
    $('profile-save-btn').disabled = false;
    return;
  }

  myAvatarUrl = `${SUPABASE_URL}/storage/v1/object/public/avatars/${path}?t=${Date.now()}`;
  await sb.from('profiles').update({ avatar_url: myAvatarUrl }).eq('id', myUserId);

  renderProfileBar();
  renderProfileModalAvatar();
  $('profile-save-btn').disabled = false;
}

// ── Signaling ─────────────────────────────────────────────────────────────────
function connectSignaling() {
  sigChannel = sb.channel(CHANNEL_NAME, { config: { broadcast: { self: false } } });

  sigChannel
    .on('broadcast', { event: 'peer_join'      }, ({ payload }) => onPeerJoin(payload))
    .on('broadcast', { event: 'peer_leave'     }, ({ payload }) => onPeerLeave(payload))
    .on('broadcast', { event: 'chat_message'   }, ({ payload }) => onChatMessage(payload))
    .on('broadcast', { event: 'chat_media'     }, ({ payload }) => onChatMedia(payload))
    .on('broadcast', { event: 'voice_state'    }, ({ payload }) => onVoiceState(payload))
    .on('broadcast', { event: 'profile_update' }, ({ payload }) => onProfileUpdate(payload))
    .on('broadcast', { event: 'sdp_offer'      }, ({ payload }) => onSdpOffer(payload))
    .on('broadcast', { event: 'sdp_answer'     }, ({ payload }) => onSdpAnswer(payload))
    .subscribe(async status => {
      if (status !== 'SUBSCRIBED') return;
      sysMsg('Connected to VoxLink. Waiting for peers…');
      await bcast('peer_join', { from: myUsername, avatar_url: myAvatarUrl, description: myDescription });
      await fetchHistory();
    });
}

function bcast(event, payload) {
  return sigChannel.send({ type: 'broadcast', event, payload });
}

// ── Peer presence ─────────────────────────────────────────────────────────────
async function onPeerJoin({ from, avatar_url, description }) {
  if (!from || from === myUsername) return;

  const isNew = !peers[from];
  peers[from] = {
    ...peers[from],
    avatarUrl:   avatar_url   ?? peers[from]?.avatarUrl   ?? null,
    description: description  ?? peers[from]?.description ?? '',
    inVoice:     peers[from]?.inVoice    ?? false,
    isSpeaking:  peers[from]?.isSpeaking ?? false,
  };

  if (isNew) sysMsg(`${from} joined the room.`);

  if (!respondedTo.has(from)) {
    respondedTo.add(from);
    await bcast('peer_join', { from: myUsername, avatar_url: myAvatarUrl, description: myDescription });
    if (myUsername < from && inVoice) await initiateCall(from);
  }

  renderMembers();
  renderVoiceParticipants();
}

function onPeerLeave({ from }) {
  if (!from || from === myUsername) return;
  delete peers[from];
  respondedTo.delete(from);
  if (peerConns[from]) { peerConns[from].close(); delete peerConns[from]; }
  removeAudio(from);
  sysMsg(`${from} left the room.`);
  renderMembers();
  renderVoiceParticipants();
}

function onChatMessage({ from, content }) {
  if (!from || !content) return;
  appendMsg(from, content);
}

// Real-time media message from desktop or another web user
function onChatMedia({ from, content, url, kind, filename }) {
  if (!from || !url) return;
  appendMsg(from, content || '', new Date(), true, { url, kind: kind || 'image', filename: filename || 'attachment' });
}

function onVoiceState({ from, speaking, muted, in_voice }) {
  if (!from || from === myUsername) return;
  if (!peers[from]) peers[from] = { inVoice: false, isSpeaking: false, avatarUrl: null, description: '' };
  peers[from].inVoice    = !!in_voice;
  peers[from].isSpeaking = !!speaking && !muted;
  renderMembers();
  renderVoiceParticipants();
}

function onProfileUpdate({ from, new_username, avatar_url, description }) {
  if (!from || from === myUsername) return;
  const old = peers[from];
  if (!old) return;
  const target = new_username ?? from;
  peers[target] = { ...old, avatarUrl: avatar_url ?? old.avatarUrl, description: description ?? old.description };
  if (target !== from) delete peers[from];
  renderMembers();
}

// ── WebRTC ────────────────────────────────────────────────────────────────────
async function getOrCreatePc(username) {
  if (peerConns[username]) return peerConns[username];

  const pc = new RTCPeerConnection(RTC_CONFIG);
  peerConns[username] = pc;

  if (localStream) {
    for (const track of localStream.getAudioTracks()) pc.addTrack(track, localStream);
  }

  const remoteStream = new MediaStream();
  const audio = document.createElement('audio');
  audio.id = `audio-${username}`;
  audio.autoplay = true;
  audio.srcObject = remoteStream;
  $('audio-elements').appendChild(audio);

  pc.ontrack = e => { e.streams[0]?.getTracks().forEach(t => remoteStream.addTrack(t)); };
  pc.oniceconnectionstatechange = () => {
    if (pc.iceConnectionState === 'disconnected' || pc.iceConnectionState === 'failed') {
      pc.close(); delete peerConns[username]; removeAudio(username);
    }
  };

  return pc;
}

function removeAudio(u) { document.getElementById(`audio-${u}`)?.remove(); }

async function initiateCall(username) {
  const pc = await getOrCreatePc(username);
  const offer = await pc.createOffer();
  await pc.setLocalDescription(offer);
  await waitIce(pc);
  await bcast('sdp_offer', { from: myUsername, to: username, sdp: pc.localDescription.sdp });
}

async function onSdpOffer({ from, to, sdp }) {
  if (to !== myUsername) return;
  const pc = await getOrCreatePc(from);
  await pc.setRemoteDescription({ type: 'offer', sdp });
  const answer = await pc.createAnswer();
  await pc.setLocalDescription(answer);
  await waitIce(pc);
  await bcast('sdp_answer', { from: myUsername, to: from, sdp: pc.localDescription.sdp });
}

async function onSdpAnswer({ from, to, sdp }) {
  if (to !== myUsername) return;
  const pc = peerConns[from];
  if (pc && pc.signalingState === 'have-local-offer') await pc.setRemoteDescription({ type: 'answer', sdp });
}

function waitIce(pc) {
  if (pc.iceGatheringState === 'complete') return Promise.resolve();
  return Promise.race([
    new Promise(r => { pc.onicegatheringstatechange = () => { if (pc.iceGatheringState === 'complete') r(); }; }),
    new Promise(r => setTimeout(r, 5000)),
  ]);
}

// ── Voice ─────────────────────────────────────────────────────────────────────
async function toggleVoice() {
  if (inVoice) await leaveVoice();
  else          await joinVoice();
}

async function joinVoice() {
  try {
    localStream = await navigator.mediaDevices.getUserMedia({ audio: true, video: false });
  } catch (err) {
    sysMsg(`Microphone access denied: ${err.message}`);
    return;
  }

  inVoice = true;

  for (const [username, pc] of Object.entries(peerConns)) {
    for (const track of localStream.getAudioTracks()) {
      if (!pc.getSenders().some(s => s.track === track)) pc.addTrack(track, localStream);
    }
    if (myUsername < username) await initiateCall(username);
  }

  for (const username of Object.keys(peers)) {
    if (!peerConns[username] && myUsername < username) await initiateCall(username);
  }

  startSpeakingDetection();
  await bcast('voice_state', { from: myUsername, speaking: false, muted: isMuted, in_voice: true });
  renderVoiceUI();
}

async function leaveVoice() {
  inVoice = false; amSpeaking = false; silenceFrames = 0;
  if (speakingTimer) { clearInterval(speakingTimer); speakingTimer = null; }
  if (audioCtx)      { audioCtx.close();             audioCtx      = null; }
  if (localStream)   { localStream.getTracks().forEach(t => t.stop()); localStream = null; }
  await bcast('voice_state', { from: myUsername, speaking: false, muted: isMuted, in_voice: false });
  renderVoiceUI();
}

function toggleMute() {
  if (!localStream) return;
  isMuted = !isMuted;
  localStream.getAudioTracks().forEach(t => { t.enabled = !isMuted; });
  bcast('voice_state', { from: myUsername, speaking: amSpeaking && !isMuted, muted: isMuted, in_voice: true });
  renderVoiceUI();
}

function startSpeakingDetection() {
  audioCtx = new AudioContext();
  const analyser = audioCtx.createAnalyser();
  analyser.fftSize = 512;
  audioCtx.createMediaStreamSource(localStream).connect(analyser);
  const buf = new Float32Array(analyser.fftSize);

  speakingTimer = setInterval(() => {
    analyser.getFloatTimeDomainData(buf);
    let sum = 0; for (const v of buf) sum += v * v;
    const rms = Math.sqrt(sum / buf.length);

    if (rms > SPEAKING_THRESHOLD) {
      silenceFrames = 0;
      if (!amSpeaking && !isMuted) {
        amSpeaking = true;
        bcast('voice_state', { from: myUsername, speaking: true, muted: false, in_voice: true });
        renderVoiceParticipants();
      }
    } else {
      silenceFrames++;
      if (amSpeaking && silenceFrames >= SILENCE_HOLD_FRAMES) {
        amSpeaking = false;
        bcast('voice_state', { from: myUsername, speaking: false, muted: isMuted, in_voice: true });
        renderVoiceParticipants();
      }
    }
  }, 100);
}

// ── Messages ──────────────────────────────────────────────────────────────────
async function fetchHistory() {
  const { data } = await sb
    .from('messages')
    .select('from_user, content, attachment_url, attachment_kind, attachment_filename, created_at')
    .eq('channel', 'general')
    .order('created_at', { ascending: false })
    .limit(100);

  if (!data) return;
  lastMsgAuthor = null;
  $('messages').innerHTML = '';

  for (const row of [...data].reverse()) {
    if (!row.from_user) continue; // skip malformed rows
    const att = row.attachment_url
      ? { url: row.attachment_url, kind: row.attachment_kind || 'image', filename: row.attachment_filename || 'attachment' }
      : null;
    appendMsg(row.from_user, row.content || '', new Date(row.created_at), false, att);
  }
  scrollBottom();
}

function trySend() {
  const input   = $('message-input');
  const content = input.textContent.trim();
  if (!content) return;
  input.textContent = '';
  $('input-placeholder').style.display = '';

  bcast('chat_message', { from: myUsername, content });
  appendMsg(myUsername, content);
  sb.from('messages').insert({ from_user: myUsername, content, channel: 'general' });
}

async function uploadMedia(file) {
  const ext      = file.name.split('.').pop().toLowerCase() || 'bin';
  const safeName = file.name.replace(/[^a-zA-Z0-9._-]/g, '_').slice(0, 40);
  const path     = `chat/${myUserId}/${Date.now()}-${safeName}`;

  $('attach-btn').disabled    = true;
  $('attach-btn').textContent = '…';

  const { error } = await sb.storage.from('avatars').upload(path, file, { contentType: file.type });

  $('attach-btn').disabled    = false;
  $('attach-btn').textContent = '+';

  if (error) { sysMsg(`Upload failed: ${error.message}`); return; }

  const url      = `${SUPABASE_URL}/storage/v1/object/public/avatars/${path}`;
  const kind     = kindForMime(file.type);
  const filename = file.name;
  const caption  = $('message-input').textContent.trim();
  $('message-input').textContent = '';
  $('input-placeholder').style.display = '';

  const att = { url, kind, filename };
  appendMsg(myUsername, caption, new Date(), true, att);

  await bcast('chat_media', { from: myUsername, content: caption, url, kind, filename });
  sb.from('messages').insert({
    from_user: myUsername, content: caption, channel: 'general',
    attachment_url: url, attachment_kind: kind, attachment_filename: filename,
  });
}

function kindForMime(mime) {
  if (mime.startsWith('audio/')) return 'audio';
  if (mime.startsWith('video/')) return 'video';
  return 'image';
}

// ── Rendering ─────────────────────────────────────────────────────────────────
function appendMsg(from, content, ts = new Date(), scroll = true, attachment = null) {
  const container  = $('messages');
  const showHeader = from !== lastMsgAuthor;
  lastMsgAuthor    = from;

  if (from === '__system') {
    const div = document.createElement('div');
    div.className   = 'system-msg';
    div.textContent = content;
    container.appendChild(div);
    if (scroll) scrollBottom();
    return;
  }

  let target; // the element to append content/media into

  if (showHeader) {
    const group = document.createElement('div');
    group.className = 'msg-group';

    const isOwn = from === myUsername;
    const color = isOwn ? '#5865f2' : '#f2f3f5';
    const bg    = avatarColor(from);
    const peer  = peers[from];
    const avUrl = isOwn ? myAvatarUrl : (peer?.avatarUrl ?? null);

    const avatarEl = document.createElement('div');
    avatarEl.className = 'msg-avatar';
    avatarEl.style.background = bg;
    avatarEl.innerHTML = avUrl ? `<img src="${esc(avUrl)}" alt="" loading="lazy">` : esc((from[0] ?? '?').toUpperCase());
    group.appendChild(avatarEl);

    const header = document.createElement('div');
    header.className = 'msg-header';
    header.innerHTML = `<span class="msg-author" style="color:${color}">${esc(from)}</span><span class="msg-time">${fmtTime(ts)}</span>`;
    group.appendChild(header);

    container.appendChild(group);
    target = group;
  } else {
    target = container.lastElementChild ?? container;
  }

  if (content) {
    const div = document.createElement('div');
    div.className   = 'msg-content';
    div.textContent = content;
    target.appendChild(div);
  }

  if (attachment?.url) {
    const wrap = document.createElement('div');
    wrap.className = 'msg-content';
    if (attachment.kind === 'audio') {
      wrap.innerHTML = `<audio controls class="msg-audio" src="${esc(attachment.url)}"></audio>`;
    } else if (attachment.kind === 'video') {
      wrap.innerHTML = `<video controls class="msg-video" src="${esc(attachment.url)}"></video>`;
    } else {
      wrap.innerHTML = `<img class="msg-img" src="${esc(attachment.url)}" alt="${esc(attachment.filename)}" loading="lazy">`;
    }
    target.appendChild(wrap);
  }

  if (scroll) scrollBottom();
}

function sysMsg(text) { appendMsg('__system', text); }

function renderMembers() {
  const list  = $('member-list');
  const count = Object.keys(peers).length + 1;
  $('members-header').textContent = `ONLINE  ${count}`;
  $('peer-count').textContent     = `${count} online`;
  list.innerHTML = '';
  list.appendChild(makeMemberRow(myUsername, myAvatarUrl, true,  inVoice, amSpeaking && !isMuted));
  for (const [u, p] of Object.entries(peers)) {
    list.appendChild(makeMemberRow(u, p.avatarUrl, false, p.inVoice, p.isSpeaking));
  }
}

function makeMemberRow(username, avatarUrl, isSelf, voiceActive, speaking) {
  const div  = document.createElement('div');
  div.className = `member-row${speaking ? ' speaking' : ''}`;
  const bg   = avatarColor(username);
  const ring = speaking ? ';box-shadow:0 0 0 2px #23a55a' : '';
  const inner = avatarUrl
    ? `<img src="${esc(avatarUrl)}" alt="" loading="lazy">`
    : esc((username[0] ?? '?').toUpperCase());
  div.innerHTML = `
    <div class="avatar avatar-sm" style="background:${bg}${ring}">${inner}</div>
    <div class="member-info">
      <div class="member-name">${esc(username)}${isSelf ? ' <span style="color:var(--text-muted);font-size:11px">(You)</span>' : ''}</div>
      <div class="member-status" style="color:${voiceActive ? 'var(--green)' : 'var(--text-muted)'}">${voiceActive ? 'In voice' : 'Online'}</div>
    </div>`;
  return div;
}

function renderVoiceParticipants() {
  const c = $('voice-participants');
  const voicePeers = Object.entries(peers).filter(([, p]) => p.inVoice);
  if (!inVoice && voicePeers.length === 0) { c.innerHTML = ''; return; }
  c.innerHTML = '';
  if (inVoice) c.appendChild(makeVoiceRow(myUsername, myAvatarUrl, amSpeaking && !isMuted));
  for (const [u, p] of voicePeers) c.appendChild(makeVoiceRow(u, p.avatarUrl, p.isSpeaking));
}

function makeVoiceRow(username, avatarUrl, speaking) {
  const div   = document.createElement('div');
  div.className = `voice-participant${speaking ? ' speaking' : ''}`;
  const bg    = avatarColor(username);
  const ring  = speaking ? ';box-shadow:0 0 0 2px #23a55a' : '';
  const inner = avatarUrl
    ? `<img src="${esc(avatarUrl)}" alt="" style="width:20px;height:20px;border-radius:50%;object-fit:cover">`
    : esc((username[0] ?? '?').toUpperCase());
  div.innerHTML = `
    <div class="avatar avatar-xs" style="background:${bg}${ring}">${inner}</div>
    <span>${esc(username)}</span>`;
  return div;
}

function renderVoiceUI() {
  const btn = $('voice-toggle-btn');
  const dot = $('voice-indicator');
  if (inVoice) {
    btn.textContent = 'Leave'; btn.className = 'voice-join-btn leave';
    dot.className   = 'voice-dot live';
    $('voice-row').className = 'channel-item active';
    $('mute-btn').style.display = '';
    $('mute-btn').className     = `icon-btn${isMuted ? ' muted' : ''}`;
    $('mute-btn').title         = isMuted ? 'Unmute' : 'Mute';
    $('mute-btn').textContent   = isMuted ? '🔇' : '🎙';
    $('self-status-bar').textContent = 'In voice';
    $('self-status-bar').style.color = 'var(--green)';
  } else {
    btn.textContent = 'Join'; btn.className = 'voice-join-btn';
    dot.className   = 'voice-dot';
    $('voice-row').className = 'channel-item';
    $('mute-btn').style.display = 'none';
    $('self-status-bar').textContent = 'Online';
    $('self-status-bar').style.color = '';
  }
  renderVoiceParticipants();
}

function renderProfileBar() {
  const bg    = avatarColor(myUsername ?? 'User');
  const inner = myAvatarUrl
    ? `<img src="${esc(myAvatarUrl)}" alt="" loading="lazy" style="width:100%;height:100%;object-fit:cover;border-radius:50%">`
    : esc((myUsername?.[0] ?? 'U').toUpperCase());
  $('self-avatar-bar').style.background = bg;
  $('self-avatar-bar').innerHTML        = inner;
  $('self-username-bar').textContent    = myUsername ?? 'User';
}

// ── Helpers ───────────────────────────────────────────────────────────────────
function showScreen(id) {
  document.querySelectorAll('.screen').forEach(s => { s.style.display = 'none'; });
  $(id).style.display = 'flex';
}

function scrollBottom() {
  const m = $('messages'); m.scrollTop = m.scrollHeight;
}

function avatarColor(username) {
  let h = 0;
  for (const c of username ?? '') h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return AVATAR_PALETTE[h % AVATAR_PALETTE.length];
}

function esc(str) {
  return String(str ?? '')
    .replace(/&/g, '&amp;').replace(/</g, '&lt;')
    .replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}

function fmtTime(date) {
  return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}

// ── Bootstrap ─────────────────────────────────────────────────────────────────
init();
