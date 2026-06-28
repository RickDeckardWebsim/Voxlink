// ─────────────────────────────────────────────────────────────────────────────
// VoxLink Web Client — vanilla JS, no build step
// ─────────────────────────────────────────────────────────────────────────────

import { createClient } from 'https://esm.sh/@supabase/supabase-js@2';
import { SUPABASE_URL, SUPABASE_ANON_KEY, SIGNALING_CHANNEL, DEFAULT_DB_CHANNEL, EVENTS, mimeToKind } from './contract.js';

const SPEAKING_THRESHOLD  = 0.01;
const SILENCE_HOLD_FRAMES = 8;   // × 100 ms = 800 ms

const RTC_CONFIG = {
  iceServers: [
    { urls: 'stun:stun.l.google.com:19302' },
    { urls: 'stun:stun1.l.google.com:19302' },
  ],
};

const AVATAR_PALETTE = [
  '#5865f2','#3ba55d','#eb459e','#f0b232',
  '#ed4245','#17a8e3','#9c59d1','#1abc9c',
];

// ── Theme defaults (must mirror :root in style.css) ───────────────────────────
const THEME_DEFAULTS = {
  '--dark-bg':     '#1e1f22',
  '--sidebar-bg':  '#2b2d31',
  '--header-bg':   '#232428',
  '--logo-badge':  '#5865f2',
  '--logo-text':   '#ffffff',
  '--input-bg':    '#484b54',
  '--input-text':  '#dbdee1',
  '--header-text':  '#dbdee1',
  '--mention-color':'#5865f2',
};

function hexToRgba(hex, alpha) {
  const h = hex.replace('#', '');
  const r = parseInt(h.slice(0, 2), 16);
  const g = parseInt(h.slice(2, 4), 16);
  const b = parseInt(h.slice(4, 6), 16);
  return `rgba(${r},${g},${b},${alpha})`;
}

// Compute the semi-transparent mention background from --mention-color.
// Replaces color-mix() which has poor browser support (Chrome 111+).
function updateMentionBg() {
  const color = getComputedStyle(document.documentElement).getPropertyValue('--mention-color').trim() || '#5865f2';
  document.documentElement.style.setProperty('--mention-bg', hexToRgba(color, 0.15));
}

function loadTheme() {
  const saved = JSON.parse(localStorage.getItem('voxlink_theme') ?? '{}');
  for (const [v, def] of Object.entries(THEME_DEFAULTS)) {
    document.documentElement.style.setProperty(v, saved[v] ?? def);
  }
  updateMentionBg();
}

function setThemeColor(cssVar, value) {
  document.documentElement.style.setProperty(cssVar, value);
  const saved = JSON.parse(localStorage.getItem('voxlink_theme') ?? '{}');
  saved[cssVar] = value;
  localStorage.setItem('voxlink_theme', JSON.stringify(saved));
  if (cssVar === '--mention-color') updateMentionBg();
}

function resetThemeColor(cssVar) {
  document.documentElement.style.setProperty(cssVar, THEME_DEFAULTS[cssVar]);
  const saved = JSON.parse(localStorage.getItem('voxlink_theme') ?? '{}');
  delete saved[cssVar];
  localStorage.setItem('voxlink_theme', JSON.stringify(saved));
  if (cssVar === '--mention-color') updateMentionBg();
}

// Apply saved theme immediately (before first paint)
loadTheme();

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
let inspectedUser = null;
const typingUsers = {};  // username → last-ping timestamp
let lastTypingPing = 0;
const QUICK_EMOJIS = ['👍', '❤️', '😂', '😮', '😢', '🙏'];
let pendingReply = null;              // { dbId, author, content } when composing a reply
let knownUsers = new Set();           // usernames online — used for @mention highlighting
let notificationAudio = null;

// ── DOM shortcuts ─────────────────────────────────────────────────────────────
const $ = id => document.getElementById(id);

// ── Init ──────────────────────────────────────────────────────────────────────
async function init() {
  bindEvents();
  const { data: { session } } = await sb.auth.getSession();
  if (session) await enterChat(session);
  else showScreen('login-screen');

  if (!$('typing-bar')) {
    const bar = document.createElement('div');
    bar.id = 'typing-bar';
    bar.className = 'typing-bar';
    const inputBar = $('input-wrap') || $('send-btn')?.closest('.input-wrap') || $('messages')?.nextElementSibling;
    if (inputBar && inputBar.parentNode) {
      inputBar.parentNode.insertBefore(bar, inputBar);
    } else {
      document.body.appendChild(bar);
    }
  }
  setInterval(renderTypingBar, 1000);
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
  $('logout-btn').addEventListener('click', e => { e.stopPropagation(); handleLogout(); });
  $('mute-btn').addEventListener('click', e => { e.stopPropagation(); toggleMute(); });
  $('voice-toggle-btn').addEventListener('click', e => { e.stopPropagation(); toggleVoice(); });
  $('send-btn').addEventListener('click', trySend);

  const input = $('message-input');
  const ph = $('input-placeholder');
  input.addEventListener('input', () => {
    ph.style.display = input.textContent.trim() ? 'none' : '';
    const now = Date.now();
    if (input.textContent.trim().length >= 2 && now - lastTypingPing > 3000) {
      lastTypingPing = now;
      bcast(EVENTS.TYPING, { from: myUsername, is_typing: true });
    }
    // @username autocomplete — detect a partial @token at the cursor and
    // filter knownUsers against it.
    const token = getMentionToken(input.textContent);
    if (token) {
      const partial = token.partial.toLowerCase();
      const matches = [...knownUsers].filter(u => u.toLowerCase().startsWith(partial)).sort();
      if (matches.length) showMentionDropdown(matches, input);
      else hideMentionDropdown();
    } else {
      hideMentionDropdown();
    }
  });
  input.addEventListener('keydown', e => {
    const dd = $('mention-dropdown');
    if (dd && dd.style.display === 'block') {
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        moveMentionSelection(dd, 1);
        return;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        moveMentionSelection(dd, -1);
        return;
      }
      if (e.key === 'Enter' || e.key === 'Tab') {
        const items = dd.querySelectorAll('.mention-item');
        if (items.length) {
          e.preventDefault();
          items[dd._mentionIndex ?? 0].click();
        }
        return;
      }
      if (e.key === 'Escape') {
        e.preventDefault();
        hideMentionDropdown();
        return;
      }
    }
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); trySend(); }
  });
  // Close the dropdown when focus leaves the input (slight delay so a click
  // on a dropdown item registers before blur hides it).
  input.addEventListener('blur', () => { setTimeout(hideMentionDropdown, 150); });

  // Media attach
  $('attach-btn').addEventListener('click', () => $('file-input').click());
  $('file-input').addEventListener('change', e => {
    if (e.target.files[0]) uploadMedia(e.target.files[0]);
    e.target.value = '';
  });

  // Profile bar → edit modal
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

  // Appearance colour pickers — live preview
  document.querySelectorAll('.color-pick-input').forEach(inp => {
    inp.addEventListener('input', e => setThemeColor(e.target.dataset.var, e.target.value));
  });
  document.querySelectorAll('.color-pick-reset').forEach(btn => {
    btn.addEventListener('click', e => {
      const v = e.target.dataset.var;
      resetThemeColor(v);
      const picker = document.querySelector(`.color-pick-input[data-var="${CSS.escape(v)}"]`);
      if (picker) picker.value = THEME_DEFAULTS[v];
    });
  });

  // Inspect card
  $('inspect-close-btn').addEventListener('click', closeInspectPanel);

  // Close inspect on ESC or click outside
  document.addEventListener('keydown', e => { if (e.key === 'Escape') { closeInspectPanel(); closeProfileModal(); pendingReply = null; hideReplyPreview(); } });
  document.addEventListener('click', e => {
    if ($('inspect-card').style.display === 'none') return;
    if ($('inspect-card').contains(e.target)) return;
    if (e.target.closest('.member-row')) return;
    closeInspectPanel();
  });

  // Hamburger menu (mobile only — button is display:none on desktop)
  const hamburger = $('hamburger-btn');
  if (hamburger) {
    hamburger.addEventListener('click', e => {
      e.stopPropagation();
      $('sidebar').classList.toggle('open');
      $('sidebar-backdrop').classList.toggle('show');
    });
  }
  const backdrop = $('sidebar-backdrop');
  if (backdrop) {
    backdrop.addEventListener('click', () => {
      $('sidebar').classList.remove('open');
      backdrop.classList.remove('show');
    });
  }

  // Announce departure when the tab/window closes. navigator.sendBeacon is
  // not usable for Realtime broadcasts (it needs the SDK channel), so we fire
  // a best-effort async broadcast; the browser may not wait for it to finish,
  // but Supabase presence timeout is the fallback.
  window.addEventListener('beforeunload', () => {
    if (sigChannel && myUsername) {
      try { bcast(EVENTS.PEER_LEAVE, { from: myUsername }); }
      catch (_) {}
    }
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
    if (!data.session) {
      // Email verification is enabled on this project — user must confirm before signing in.
      $('login-error').style.color = '#23a55a';
      $('login-error').textContent = 'Account created! Check your email to confirm, then sign in.';
      $('login-btn').disabled = false;
      $('login-btn').textContent = 'Sign In';
      authMode = 'signin';
      $('username-row').style.display   = 'none';
      $('auth-toggle-text').textContent = "Don't have an account?";
      $('auth-toggle-link').textContent = 'Create one';
      return;
    }
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
  knownUsers = new Set([myUsername]);   // reset + seed own name for @mention highlighting
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
  // Announce departure so peers update their sidebars without waiting for
  // Realtime presence timeout. Best-effort: a dying socket may reject the send.
  if (sigChannel && myUsername) {
    try { await bcast(EVENTS.PEER_LEAVE, { from: myUsername }); }
    catch (_) { /* socket already closing — presence timeout will catch it */ }
  }
  if (sigChannel) { sigChannel.unsubscribe(); sigChannel = null; }
  for (const pc of Object.values(peerConns)) pc.close();
  Object.keys(peerConns).forEach(k => delete peerConns[k]);
  Object.keys(peers).forEach(k => delete peers[k]);
  respondedTo.clear();
  knownUsers = new Set();
}

// ── Inspect panel ─────────────────────────────────────────────────────────────
function openInspectPanel(username, clickY) {
  const isSelf = username === myUsername;
  const avUrl  = isSelf ? myAvatarUrl           : (peers[username]?.avatarUrl  ?? null);
  const desc   = isSelf ? (myDescription ?? '') : (peers[username]?.description ?? '');

  const avEl = $('inspect-av');
  avEl.style.background = avatarColor(username);
  avEl.innerHTML = avUrl
    ? `<img src="${esc(avUrl)}" alt="" style="width:100%;height:100%;object-fit:cover;border-radius:50%">`
    : esc((username[0] ?? '?').toUpperCase());

  $('inspect-name').textContent = username;

  if (desc) {
    $('inspect-desc').textContent    = desc;
    $('inspect-about').style.display = '';
  } else {
    $('inspect-about').style.display = 'none';
  }

  const card = $('inspect-card');
  card.style.display = 'block';
  const cardH = card.offsetHeight || 120;
  const top   = Math.max(8, Math.min(clickY - 16, window.innerHeight - cardH - 8));
  card.style.top = `${top}px`;
  inspectedUser = username;
}

function closeInspectPanel() {
  $('inspect-card').style.display = 'none';
  inspectedUser = null;
}

// ── Profile modal ─────────────────────────────────────────────────────────────
function openProfileModal() {
  $('profile-username-input').value    = myUsername    ?? '';
  $('profile-desc-input').value        = myDescription ?? '';
  $('profile-desc-count').textContent  = ($('profile-desc-input').value.length).toString();
  $('profile-modal-error').textContent = '';
  renderProfileModalAvatar();
  syncColorPickers();
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

function syncColorPickers() {
  const saved = JSON.parse(localStorage.getItem('voxlink_theme') ?? '{}');
  for (const [v, def] of Object.entries(THEME_DEFAULTS)) {
    const picker = document.querySelector(`.color-pick-input[data-var="${CSS.escape(v)}"]`);
    if (picker) picker.value = saved[v] ?? def;
  }
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

  await bcast(EVENTS.PROFILE_UPDATE, {
    from: oldUsername, new_username: myUsername,
    avatar_url: myAvatarUrl, description: myDescription,
  });

  btn.disabled = false; btn.textContent = 'Save Changes';
  closeProfileModal();
}

async function uploadProfileAvatar(file) {
  const ext  = file.name.split('.').pop().toLowerCase() || 'png';
  const path = `${myUserId}_avatar.${ext}`;

  $('profile-save-btn').disabled       = true;
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
  sigChannel = sb.channel(SIGNALING_CHANNEL, { config: { broadcast: { self: false } } });

  sigChannel
    .on('broadcast', { event: EVENTS.PEER_JOIN      }, ({ payload }) => onPeerJoin(payload))
    .on('broadcast', { event: EVENTS.PEER_LEAVE     }, ({ payload }) => onPeerLeave(payload))
    .on('broadcast', { event: EVENTS.CHAT_MESSAGE   }, ({ payload }) => onChatMessage(payload))
    .on('broadcast', { event: EVENTS.CHAT_MEDIA     }, ({ payload }) => onChatMedia(payload))
    .on('broadcast', { event: EVENTS.VOICE_STATE    }, ({ payload }) => onVoiceState(payload))
    .on('broadcast', { event: EVENTS.PROFILE_UPDATE }, ({ payload }) => onProfileUpdate(payload))
    .on('broadcast', { event: EVENTS.TYPING         }, ({ payload }) => onTyping(payload))
    .on('broadcast', { event: EVENTS.REACTION       }, ({ payload }) => onReaction(payload))
    .on('broadcast', { event: EVENTS.SDP_OFFER      }, ({ payload }) => onSdpOffer(payload))
    .on('broadcast', { event: EVENTS.SDP_ANSWER     }, ({ payload }) => onSdpAnswer(payload))
    .subscribe(async status => {
      if (status !== 'SUBSCRIBED') return;
      sysMsg('Connected to VoxLink. Waiting for peers…');
      await bcast(EVENTS.PEER_JOIN, { from: myUsername, avatar_url: myAvatarUrl, description: myDescription });
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
    avatarUrl:   avatar_url  ?? peers[from]?.avatarUrl   ?? null,
    description: description ?? peers[from]?.description ?? '',
    inVoice:     peers[from]?.inVoice    ?? false,
    isSpeaking:  peers[from]?.isSpeaking ?? false,
  };
  knownUsers.add(from);

  if (isNew) sysMsg(`${from} joined the room.`);

  if (!respondedTo.has(from)) {
    respondedTo.add(from);
    await bcast(EVENTS.PEER_JOIN, { from: myUsername, avatar_url: myAvatarUrl, description: myDescription });
    if (myUsername < from && inVoice) await initiateCall(from);
  }

  renderMembers();
  renderVoiceParticipants();
}

function onPeerLeave({ from }) {
  if (!from || from === myUsername) return;
  delete peers[from];
  knownUsers.delete(from);
  respondedTo.delete(from);
  if (peerConns[from]) { peerConns[from].close(); delete peerConns[from]; }
  removeAudio(from);
  sysMsg(`${from} left the room.`);
  if (inspectedUser === from) closeInspectPanel();
  renderMembers();
  renderVoiceParticipants();
}

function onChatMessage({ from, content, message_id, reply_to, reply_to_author, reply_to_content }) {
  if (!from || !content) return;
  const reply = reply_to ? { reply_to, reply_to_author, reply_to_content } : null;
  appendMsg(from, content, new Date(), true, null, message_id || null, reply);
  if (mentionsUser(content, myUsername)) { playNotification(); showPingToast(from, content); }
}

function onChatMedia({ from, content, url, kind, filename, message_id, reply_to, reply_to_author, reply_to_content }) {
  if (!from || !url) return;
  const reply = reply_to ? { reply_to, reply_to_author, reply_to_content } : null;
  appendMsg(from, content || '', new Date(), true, { url, kind: kind || 'image', filename: filename || 'attachment' }, message_id || null, reply);
  if (mentionsUser(content || '', myUsername)) { playNotification(); showPingToast(from, content || ''); }
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
  if (target !== from) { delete peers[from]; knownUsers.delete(from); knownUsers.add(target); }
  if (inspectedUser === from && target !== from) closeInspectPanel();
  renderMembers();
}


function onTyping({ from, is_typing }) {
  if (!from || from === myUsername) return;
  if (is_typing) {
    typingUsers[from] = Date.now();
  } else {
    delete typingUsers[from];
  }
  renderTypingBar();
}

function renderTypingBar() {
  const bar = $('typing-bar');
  const now = Date.now();
  for (const [u, ts] of Object.entries(typingUsers)) {
    if (now - ts > 4000) delete typingUsers[u];
  }
  const users = Object.keys(typingUsers);
  if (users.length === 0) {
    if (bar) bar.textContent = '';
    return;
  }
  const text = users.length === 1 ? `${users[0]} is typing…`
             : users.length === 2 ? `${users[0]} and ${users[1]} are typing…`
             : 'Several people are typing…';
  if (bar) bar.textContent = text;
}

function onReaction({ from, message_id, emoji, active }) {
  if (!from || from === myUsername || !message_id || !emoji) return;
  const row = document.querySelector(`[data-msg-id="${CSS.escape(message_id)}"]`);
  if (!row) return;
  updateReactionPills(row, from, emoji, active);
}

function updateReactionPills(row, user, emoji, active) {
  let pillsEl = row.querySelector('.msg-reactions');
  if (!pillsEl) {
    pillsEl = document.createElement('div');
    pillsEl.className = 'msg-reactions';
    row.appendChild(pillsEl);
  }
  let reactions = pillsEl._reactions ?? {};
  const key = `${user}:${emoji}`;
  if (active) {
    reactions[key] = { user, emoji };
  } else {
    delete reactions[key];
  }
  pillsEl._reactions = reactions;

  const groups = {};
  for (const { user: u, emoji: e } of Object.values(reactions)) {
    groups[e] = groups[e] || new Set();
    groups[e].add(u);
  }

  pillsEl.innerHTML = '';
  for (const [e, users] of Object.entries(groups)) {
    const pill = document.createElement('span');
    pill.className = 'reaction-pill';
    pill.textContent = `${e} ${users.size}`;
    if (users.has(myUsername)) pill.classList.add('reacted');
    pill.onclick = () => {
      const active = !users.has(myUsername);
      toggleReaction(row.dataset.msgId, e, active);
    };
    pillsEl.appendChild(pill);
  }
}

async function toggleReaction(messageId, emoji, active) {
  const row = document.querySelector(`[data-msg-id="${CSS.escape(messageId)}"]`);
  if (row) updateReactionPills(row, myUsername, emoji, active);
  bcast(EVENTS.REACTION, { from: myUsername, message_id: messageId, emoji, active });
  if (active) {
    const { error } = await sb.from('reactions').insert({ message_id: messageId, username: myUsername, emoji });
    if (error) console.warn('Reaction insert failed:', error.message);
  } else {
    const { error } = await sb.from('reactions')
      .delete()
      .eq('message_id', messageId).eq('username', myUsername).eq('emoji', emoji);
    if (error) console.warn('Reaction delete failed:', error.message);
  }
}

function showReactionPicker(x, y, messageId, targetEl) {
  let picker = $('reaction-picker');
  if (!picker) {
    picker = document.createElement('div');
    picker.id = 'reaction-picker';
    picker.className = 'reaction-picker';
    document.body.appendChild(picker);
  }
  // Clamp to viewport so the picker doesn't overflow on mobile. The picker
  // has two rows: emoji row + reply row.
  const pw = 220, ph = 84;
  const maxX = window.innerWidth - pw - 8;
  const maxY = window.innerHeight - ph - 8;
  picker.style.left = `${Math.min(x, Math.max(0, maxX))}px`;
  picker.style.top  = `${Math.min(y, Math.max(0, maxY))}px`;
  picker.style.display = 'flex';
  picker.style.flexDirection = 'column';
  picker.innerHTML = '';
  const emojiRow = document.createElement('div');
  emojiRow.style.display = 'flex';
  emojiRow.style.flexWrap = 'wrap';
  picker.appendChild(emojiRow);
  for (const emoji of QUICK_EMOJIS) {
    const btn = document.createElement('span');
    btn.className = 'picker-emoji';
    btn.textContent = emoji;
    btn.onclick = ev => {
      ev.stopPropagation();
      const pillsEl = targetEl.closest('[data-msg-id]')?.querySelector('.msg-reactions');
      const reactions = pillsEl?._reactions ?? {};
      const already = Object.values(reactions).some(r => r.user === myUsername && r.emoji === emoji);
      toggleReaction(messageId, emoji, !already);
      picker.style.display = 'none';
    };
    emojiRow.appendChild(btn);
  }
  // Reply affordance — essential on touch where the hover reply button never
  // appears. Reads author + content from the message's own DOM.
  const replyBtn = document.createElement('button');
  replyBtn.type = 'button';
  replyBtn.className = 'picker-reply';
  replyBtn.textContent = 'Reply';
  replyBtn.onclick = ev => {
    ev.stopPropagation();
    const row = targetEl.closest('[data-msg-id]');
    const group = row?.closest('.msg-group');
    const authorEl = group?.querySelector('.msg-author');
    const author = authorEl?.textContent || '';
    const contentEl = row?.querySelector('.msg-content');
    const msgContent = (contentEl?.textContent || '').slice(0, 100);
    pendingReply = { dbId: row?.dataset.msgId, author, content: msgContent };
    showReplyPreview();
    picker.style.display = 'none';
  };
  picker.appendChild(replyBtn);
  setTimeout(() => {
    document.addEventListener('click', function close() {
      picker.style.display = 'none';
      document.removeEventListener('click', close);
    }, { once: true });
  }, 0);
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
  audio.id        = `audio-${username}`;
  audio.autoplay  = true;
  audio.srcObject = remoteStream;
  $('audio-elements').appendChild(audio);

  pc.ontrack = e => {
    e.streams[0]?.getTracks().forEach(t => remoteStream.addTrack(t));
    // Explicitly play — autoplay can be blocked by browser policy, and the
    // user granting mic access counts as a gesture that allows this play() call.
    audio.play().catch(err => console.warn(`[voice] audio.play() blocked for ${username}:`, err.message));
  };
  pc.oniceconnectionstatechange = () => {
    if (pc.iceConnectionState === 'disconnected' || pc.iceConnectionState === 'failed') {
      pc.close(); delete peerConns[username]; removeAudio(username);
    }
  };

  return pc;
}

function removeAudio(u) { document.getElementById(`audio-${u}`)?.remove(); }

async function initiateCall(username) {
  const pc    = await getOrCreatePc(username);
  const offer = await pc.createOffer();
  await pc.setLocalDescription(offer);
  await waitIce(pc);
  await bcast(EVENTS.SDP_OFFER, { from: myUsername, to: username, sdp: pc.localDescription.sdp });
}

async function onSdpOffer({ from, to, sdp }) {
  if (to !== myUsername) return;
  if (!inVoice) return; // a non-voice peer must not answer voice calls
  const pc = await getOrCreatePc(from);
  await pc.setRemoteDescription({ type: 'offer', sdp });
  const answer = await pc.createAnswer();
  await pc.setLocalDescription(answer);
  await waitIce(pc);
  await bcast(EVENTS.SDP_ANSWER, { from: myUsername, to: from, sdp: pc.localDescription.sdp });
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

  // Add our mic track to any peer connections that already exist (e.g. data
  // channels created earlier). Renegotiation must be initiated by the side
  // that added the track, so we re-offer to every peer currently in voice —
  // not just those whose username sorts after ours. The lexicographic guard
  // below is kept only for the initial-call case to avoid glare.
  for (const [username, pc] of Object.entries(peerConns)) {
    for (const track of localStream.getAudioTracks()) {
      if (!pc.getSenders().some(s => s.track === track)) pc.addTrack(track, localStream);
    }
    if (peers[username]?.inVoice) await initiateCall(username);
  }

  // Initial calls to peers we don't yet have a PC with. Keep the
  // myUsername < username guard here so two peers joining simultaneously
  // don't both fire the first offer (glare).
  for (const username of Object.keys(peers)) {
    if (!peerConns[username] && myUsername < username) await initiateCall(username);
  }

  startSpeakingDetection();
  await bcast(EVENTS.VOICE_STATE, { from: myUsername, speaking: false, muted: isMuted, in_voice: true });
  renderVoiceUI();
}

async function leaveVoice() {
  inVoice = false; amSpeaking = false; silenceFrames = 0;
  if (speakingTimer) { clearInterval(speakingTimer); speakingTimer = null; }
  if (audioCtx)      { audioCtx.close();             audioCtx      = null; }
  if (localStream)   { localStream.getTracks().forEach(t => t.stop()); localStream = null; }
  await bcast(EVENTS.VOICE_STATE, { from: myUsername, speaking: false, muted: isMuted, in_voice: false });
  renderVoiceUI();
}

function toggleMute() {
  if (!localStream) return;
  isMuted = !isMuted;
  localStream.getAudioTracks().forEach(t => { t.enabled = !isMuted; });
  bcast(EVENTS.VOICE_STATE, { from: myUsername, speaking: amSpeaking && !isMuted, muted: isMuted, in_voice: true });
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
        bcast(EVENTS.VOICE_STATE, { from: myUsername, speaking: true, muted: false, in_voice: true });
        renderVoiceParticipants();
      }
    } else {
      silenceFrames++;
      if (amSpeaking && silenceFrames >= SILENCE_HOLD_FRAMES) {
        amSpeaking = false;
        bcast(EVENTS.VOICE_STATE, { from: myUsername, speaking: false, muted: isMuted, in_voice: true });
        renderVoiceParticipants();
      }
    }
  }, 100);
}
async function fetchHistory() {
  const { data } = await sb
    .from('messages')
    .select('id, from_user, content, attachment_url, attachment_kind, attachment_filename, reply_to_id, reply_to_author, reply_to_content, created_at')
    .eq('channel', DEFAULT_DB_CHANNEL)
    .order('created_at', { ascending: false })
    .limit(100);

  if (!data) return;
  lastMsgAuthor = null;
  $('messages').innerHTML = '';

  for (const row of [...data].reverse()) {
    if (!row.from_user) continue;
    const att = row.attachment_url
      ? { url: row.attachment_url, kind: row.attachment_kind || 'image', filename: row.attachment_filename || 'attachment' }
      : null;
    const reply = row.reply_to_id ? { reply_to: row.reply_to_id, reply_to_author: row.reply_to_author, reply_to_content: row.reply_to_content } : null;
    appendMsg(row.from_user, row.content || '', new Date(row.created_at), false, att, row.id, reply);
  }

  const ids = data.map(r => r.id).filter(Boolean);
  if (ids.length) {
    const { data: rxn } = await sb.from('reactions').select('message_id,username,emoji').in('message_id', ids);
    if (rxn) {
      for (const r of rxn) {
        const row = document.querySelector(`[data-msg-id="${CSS.escape(r.message_id)}"]`);
        if (row) updateReactionPills(row, r.username, r.emoji, true);
      }
    }
  }

  scrollBottom();
}

async function trySend() {
  const input   = $('message-input');
  const content = input.textContent.trim();
  if (!content) return;
  input.textContent = '';
  $('input-placeholder').style.display = '';
  bcast(EVENTS.TYPING, { from: myUsername, is_typing: false });

  const messageId = crypto.randomUUID();
  const reply = pendingReply ? {
    reply_to: pendingReply.dbId, reply_to_author: pendingReply.author, reply_to_content: pendingReply.content,
  } : {};

  bcast(EVENTS.CHAT_MESSAGE, { from: myUsername, content, message_id: messageId, ...reply });
  appendMsg(myUsername, content, new Date(), true, null, messageId, reply.reply_to ? reply : null);

  const { error } = await sb.from('messages').insert({
    id: messageId, from_user: myUsername, content, channel: DEFAULT_DB_CHANNEL,
    ...(reply.reply_to ? { reply_to_id: reply.reply_to, reply_to_author: reply.reply_to_author, reply_to_content: reply.reply_to_content } : {}),
  });
  if (error) sysMsg(`⚠ Message not saved to history: ${error.message}`);
  pendingReply = null;
  hideReplyPreview();
}

async function uploadMedia(file) {
  const safeName = file.name.replace(/[^a-zA-Z0-9._-]/g, '_').slice(0, 40);
  const path     = `chat/${myUserId}/${Date.now()}-${safeName}`;

  $('attach-btn').disabled    = true;
  $('attach-btn').textContent = '…';

  const { error: uploadErr } = await sb.storage.from('avatars').upload(path, file, { contentType: file.type });

  $('attach-btn').disabled    = false;
  $('attach-btn').textContent = '+';

  if (uploadErr) { sysMsg(`Upload failed: ${uploadErr.message}`); return; }

  const url      = `${SUPABASE_URL}/storage/v1/object/public/avatars/${path}`;
  const kind     = mimeToKind(file.type);
  const filename = file.name;
  const caption  = $('message-input').textContent.trim();
  $('message-input').textContent = '';
  $('input-placeholder').style.display = '';
  bcast(EVENTS.TYPING, { from: myUsername, is_typing: false });

  const messageId = crypto.randomUUID();
  const reply = pendingReply ? {
    reply_to: pendingReply.dbId, reply_to_author: pendingReply.author, reply_to_content: pendingReply.content,
  } : {};

  appendMsg(myUsername, caption, new Date(), true, { url, kind, filename }, messageId, reply.reply_to ? reply : null);
  await bcast(EVENTS.CHAT_MEDIA, { from: myUsername, content: caption, url, kind, filename, message_id: messageId, ...reply });

  const { error: insertErr } = await sb.from('messages').insert({
    id: messageId, from_user: myUsername, content: caption, channel: DEFAULT_DB_CHANNEL,
    attachment_url: url, attachment_kind: kind, attachment_filename: filename,
    ...(reply.reply_to ? { reply_to_id: reply.reply_to, reply_to_author: reply.reply_to_author, reply_to_content: reply.reply_to_content } : {}),
  });
  if (insertErr) sysMsg(`⚠ Media not saved to history: ${insertErr.message}`);
  pendingReply = null;
  hideReplyPreview();
}


// ── Rendering ─────────────────────────────────────────────────────────────────
function appendMsg(from, content, ts = new Date(), scroll = true, attachment = null, dbId = null, reply = null) {
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

  let target;

  if (showHeader) {
    const group = document.createElement('div');
    // NOTE: data-msg-id lives on the per-message .msg-row below, not the
    // group, so a group's continuation messages are individually addressable
    // for reactions/replies and querySelector('[data-msg-id]') hits the row.
    group.className = 'msg-group';

    const isOwn = from === myUsername;
    const color = isOwn ? '#5865f2' : '#f2f3f5';
    const bg    = avatarColor(from);
    const avUrl = isOwn ? myAvatarUrl : (peers[from]?.avatarUrl ?? null);

    const avatarEl = document.createElement('div');
    avatarEl.className        = 'msg-avatar';
    avatarEl.style.background = bg;
    avatarEl.innerHTML = avUrl
      ? `<img src="${esc(avUrl)}" alt="" loading="lazy">`
      : esc((from[0] ?? '?').toUpperCase());
    group.appendChild(avatarEl);

    const header = document.createElement('div');
    header.className = 'msg-header';
    header.innerHTML = `<span class="msg-author" style="color:${color}">${esc(from)}</span><span class="msg-time">${fmtTime(ts)}</span>`;
    group.appendChild(header);
    // Reply ref moved to per-message .msg-row below (renders for both
    // header and continuation messages).

    container.appendChild(group);
    target = group;
  } else {
    target = container.lastElementChild ?? container;
  }

  // Each individual message gets its own identity + reply affordance, so a
  // continuation message (showHeader === false) is replyable on its own and
  // carries its own dbId rather than borrowing the group's first-message id.
  let msgEl = target;
  if (dbId) {
    msgEl = document.createElement('div');
    msgEl.className = 'msg-row';
    msgEl.dataset.msgId = dbId;
    target.appendChild(msgEl);
  }

  // Reply reference — rendered on every message that has reply data, including
  // continuation messages (showHeader === false) where it was previously lost.
  if (reply && msgEl) {
    const refEl = document.createElement('div');
    refEl.className = 'msg-reply-ref';
    refEl.innerHTML = `<span class="reply-arrow">↪</span> @${esc(reply.reply_to_author)}: ${esc(reply.reply_to_content)}`;
    msgEl.appendChild(refEl);
  }

  if (content) {
    const div = document.createElement('div');
    div.className   = 'msg-content';
    div.innerHTML = highlightMentions(content);
    msgEl.appendChild(div);
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
    msgEl.appendChild(wrap);
  }

  // Hover Reply affordance — every message with a dbId gets its own button.
  if (dbId && msgEl) {
    const replyBtn = document.createElement('button');
    replyBtn.className = 'reply-btn';
    replyBtn.type = 'button';
    replyBtn.textContent = 'Reply';
    replyBtn.onclick = ev => {
      ev.stopPropagation();
      pendingReply = { dbId: msgEl.dataset.msgId, author: from, content: (content || '').slice(0, 100) };
      showReplyPreview();
    };
    msgEl.appendChild(replyBtn);
  }


  if (dbId && msgEl) {
    msgEl.addEventListener('contextmenu', e => {
      e.preventDefault();
      showReactionPicker(e.clientX, e.clientY, dbId, msgEl);
    });
    // Long-press for touch devices — contextmenu never fires on touch.
    let pressTimer = null;
    msgEl.addEventListener('touchstart', e => {
      const touch = e.touches[0];
      pressTimer = setTimeout(() => {
        e.preventDefault();
        showReactionPicker(touch.clientX, touch.clientY, dbId, msgEl);
      }, 500);
    }, { passive: true });
    msgEl.addEventListener('touchmove', () => { clearTimeout(pressTimer); }, { passive: true });
    msgEl.addEventListener('touchend', () => { clearTimeout(pressTimer); }, { passive: true });
  }
  if (scroll) scrollBottom();
}

// ── Reply + mention + notification helpers ────────────────────────────────────

// Wrap @<knownUsername> in a highlight span. HTML-escaped first, so the
// injected <span> markup is the only unescaped text in the result.
// Matches must be word-bounded so a short username like "jo" does not
// corrupt "@joseph" — and longest names are processed first so "joseph"
// wins over "jo" when both are known users.
function escapeRegex(s) { return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'); }

function highlightMentions(text) {
  let html = esc(text);
  // Sort longest-first so "joseph" is wrapped before "jo" can corrupt it.
  const users = [...knownUsers].sort((a, b) => b.length - a.length);
  for (const user of users) {
    // The username must be escaped for both the regex (so special regex chars
    // don't break the pattern) AND the replacement (so a malicious username
    // like <img onerror=...> can't inject HTML). esc() produces the same
    // encoding the text was already put through, so the regex matches
    // the escaped text and the replacement is safe to insert as innerHTML.
    const escUser = esc(user);
    const re = new RegExp('@' + escapeRegex(escUser) + '\\b', 'g');
    html = html.replace(re, `<span class="mention">@${escUser}</span>`);
  }
  return html;
}

// True if text contains an @mention of the given username at a word
// boundary (so "@joseph" does not count as a ping for user "jo").
function mentionsUser(text, username) {
  if (!username) return false;
  const re = new RegExp('@' + escapeRegex(username) + '\\b');
  return re.test(text);
}

// ── @username autocomplete dropdown ───────────────────────────────────────────

// Extract the partial @mention token the user is currently typing.
// Returns { atIdx, partial } when there's an '@' followed by zero or more
// word chars with no whitespace after it, otherwise null.
function getMentionToken(text) {
  const atIdx = text.lastIndexOf('@');
  if (atIdx === -1) return null;
  const after = text.slice(atIdx + 1);
  // A space after the '@' means it's no longer a mention in progress.
  if (/\s/.test(after)) return null;
  const match = after.match(/^[\w.-]*/);
  return { atIdx, partial: match ? match[0] : '' };
}

function showMentionDropdown(matches, inputEl) {
  const dd = $('mention-dropdown');
  if (!dd || !matches.length) { hideMentionDropdown(); return; }
  dd.innerHTML = '';
  matches.slice(0, 8).forEach((user, i) => {
    const item = document.createElement('div');
    item.className = 'mention-item' + (i === 0 ? ' selected' : '');
    item.textContent = user;
    item.onclick = () => insertMention(user, inputEl);
    dd.appendChild(item);
  });
  dd.style.display = 'block';
  dd._mentionMatches = matches;
  dd._mentionIndex = 0;
}

function hideMentionDropdown() {
  const dd = $('mention-dropdown');
  if (dd) { dd.style.display = 'none'; dd._mentionMatches = null; }
}

function moveMentionSelection(dd, delta) {
  const items = dd.querySelectorAll('.mention-item');
  if (!items.length) return;
  const n = items.length;
  let idx = (dd._mentionIndex ?? 0) + delta;
  if (idx < 0) idx = n - 1;
  if (idx >= n) idx = 0;
  dd._mentionIndex = idx;
  items.forEach((it, i) => it.classList.toggle('selected', i === idx));
  items[idx].scrollIntoView({ block: 'nearest' });
}

function insertMention(username, inputEl) {
  const text = inputEl.textContent;
  const atIdx = text.lastIndexOf('@');
  if (atIdx === -1) { hideMentionDropdown(); return; }
  const before = text.slice(0, atIdx);
  const after = text.slice(atIdx + 1).replace(/^[\w.-]*/, '');
  inputEl.textContent = before + '@' + username + ' ' + after;
  // Place the cursor right after the inserted mention + trailing space.
  const sel = window.getSelection();
  const range = document.createRange();
  inputEl.focus();
  range.selectNodeContents(inputEl);
  range.collapse(false);
  sel.removeAllRanges();
  sel.addRange(range);
  hideMentionDropdown();
  $('input-placeholder').style.display = inputEl.textContent ? 'none' : '';
}

function showReplyPreview() {
  if (!pendingReply) return;
  const bar = $('reply-preview');
  if (!bar) return;
  bar.innerHTML = `<span class="reply-arrow">↪</span> Replying to @${esc(pendingReply.author)}: ${esc(pendingReply.content)} <button class="reply-cancel" type="button">✕</button>`;
  bar.style.display = 'flex';
  bar.querySelector('.reply-cancel').onclick = () => { pendingReply = null; hideReplyPreview(); };
}

function hideReplyPreview() {
  const bar = $('reply-preview');
  if (bar) bar.style.display = 'none';
}

function playNotification() {
  if (!notificationAudio) notificationAudio = new Audio('notification.mp3');
  notificationAudio.currentTime = 0;
  notificationAudio.play().catch(() => {}); // autoplay blocks until a user gesture
}

let pingToastTimer = null;
function showPingToast(from, content) {
  const toast = $('ping-toast');
  if (!toast) return;
  // Populate avatar + text
  const av = $('ping-avatar');
  if (av) { av.style.background = avatarColor(from); av.textContent = (from[0] ?? '?').toUpperCase(); }
  const title = $('ping-title');
  if (title) title.textContent = `${from} mentioned you`;
  const preview = $('ping-preview');
  if (preview) preview.textContent = content.slice(0, 120);
  // Show with slide-in animation
  toast.classList.remove('hide');
  toast.classList.add('show');
  // Auto-dismiss after 5 seconds
  clearTimeout(pingToastTimer);
  pingToastTimer = setTimeout(() => {
    toast.classList.remove('show');
    toast.classList.add('hide');
  }, 5000);
}

// Browsers gate audio playback behind a user gesture; unlock on first
// interaction by playing silently (volume 0) then restoring volume, so the
// first touch does not blast the notification sound audibly.
document.addEventListener('pointerdown', () => {
  if (!notificationAudio) notificationAudio = new Audio('notification.mp3');
  notificationAudio.volume = 0;
  notificationAudio.play().then(() => { notificationAudio.volume = 1; }).catch(() => {});
}, { once: true });

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
  const div = document.createElement('div');
  div.className = `member-row${speaking ? ' speaking' : ''}`;

  const bg    = avatarColor(username);
  const ring  = speaking ? ';box-shadow:0 0 0 2px #23a55a' : '';
  const inner = avatarUrl
    ? `<img src="${esc(avatarUrl)}" alt="" loading="lazy">`
    : esc((username[0] ?? '?').toUpperCase());

  div.innerHTML = `
    <div class="avatar avatar-sm" style="background:${bg}${ring}">${inner}</div>
    <div class="member-info">
      <div class="member-name">${esc(username)}${isSelf ? ' <span style="color:var(--text-muted);font-size:11px">(You)</span>' : ''}</div>
      <div class="member-status" style="color:${voiceActive ? 'var(--green)' : 'var(--text-muted)'}">${voiceActive ? 'In voice' : 'Online'}</div>
    </div>`;

  // Own row opens profile edit modal; peer rows open the inspect card
  div.addEventListener('click', e => {
    e.stopPropagation();
    if (isSelf) {
      closeInspectPanel();
      openProfileModal();
    } else {
      openInspectPanel(username, e.clientY);
    }
  });

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

function scrollBottom() { const m = $('messages'); m.scrollTop = m.scrollHeight; }

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
