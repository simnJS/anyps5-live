"use strict";

// Leonida, live: the dashboard collects the repository every minute (src/leonida.rs); this page reads it every
// 10 seconds, counts the stars up, slides new merges in, and celebrates every thousand stars.
// ?demo=milestone plays the celebration.
const REFRESH_MS = 10_000, RELOAD_MS = 6 * 3_600_000, CTA_MS = 12_000, MILESTONE = 1000;
// The panel under the "Just merged" feed, in turn. Each has its own QR code; HangarAI, which built this live,
// comes with a code to its site. The subscribe line follows the boot status.
const LEONIDA_QR = { qr: "leonida-qr.svg", sub: "⭐ Scan to star it · github.com/ProjectLeonida/Leonida" };
const HANGAR_QR = { qr: "hangar-site-qr.svg", sub: "Coding agents in one window, free · hangarai.dev", hangar: true };
const CTAS = [
  { text: "⭐ Star Leonida on GitHub", ...LEONIDA_QR },
  { text: "👍 Hit like if you want GTA VI on PC", ...LEONIDA_QR },
  { text: "🤖 This live was built with HangarAI", ...HANGAR_QR },
  { text: (boots) => (boots ? "🔔 Subscribe: follow it all the way to playable" : "🔔 Subscribe: the first boot shows up here, live"), ...LEONIDA_QR },
  { text: "💬 Which GPU would you run it on? Tell the chat", ...LEONIDA_QR },
];
let bootMark = null;

// The warning band. "danger" lines are the scams; the boot line follows the README's boot row.
const TICKER_PX_PER_SECOND = 70;
const WARNINGS = (boots) => [
  { text: "Never pay for a GTA VI PC .exe: it is a scam", danger: true },
  { text: boots ? "It is not playable yet: every video of a finished GTA VI PC port is fake" : "The game does not boot yet: every video of GTA VI running on PC is fake", danger: true },
  { text: "Leonida is free and open source: it builds the game on your PC from your own copy, there is nothing to buy" },
  { text: "Never run files sent in comments, DMs, Discord or Telegram", danger: true },
  { text: "The only real source: github.com/ProjectLeonida/Leonida" },
  { text: "Unofficial fan tracker, not affiliated with Rockstar Games, Take-Two, Microsoft or the Leonida maintainers" },
  { text: "This live was built with HangarAI · hangarai.dev" },
];
let tickerKey = "";

function renderTicker() {
  const boots = Boolean(bootMark && bootMark !== "no");
  const key = String(boots);
  if (key === tickerKey) return;
  tickerKey = key;
  const once = WARNINGS(boots).map((line) => `<span class="${line.danger ? "danger" : ""}">${escape(line.text)}</span><i>•</i>`).join("");
  const strip = $("ticker");
  // Twice in a row: the strip slides by half its width and starts over where it looks the same.
  strip.innerHTML = once + once;
  strip.style.animationDuration = `${(strip.scrollWidth / 2 / TICKER_PX_PER_SECOND).toFixed(1)}s`;
}
const number = new Intl.NumberFormat("en-US");
const fmt = (value) => (value === null || value === undefined ? "—" : number.format(value));
const escape = (text) => String(text ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const $ = (id) => document.getElementById(id);
const params = new URLSearchParams(location.search);
let latest = null, shownStars = null, startedAt = null, seenPrs = null;

function tickClock() {
  $("clock").textContent = new Date().toLocaleTimeString("en-GB", { timeZone: "UTC", hour: "2-digit", minute: "2-digit", hour12: false });
}

function ago(iso) {
  const seconds = Math.max(0, (Date.now() - Date.parse(iso)) / 1000);
  if (seconds < 60) return "just now";
  if (seconds < 3600) return `${Math.floor(seconds / 60)} min ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)} h ago`;
  return `${Math.floor(seconds / 86400)} d ago`;
}

function age(iso) {
  const hours = Math.max(0, (Date.now() - Date.parse(iso)) / 3_600_000);
  const days = Math.floor(hours / 24);
  return days ? `${days} day${days > 1 ? "s" : ""} ${Math.floor(hours % 24)} h` : `${Math.floor(hours)} h`;
}

// The star count climbs to its new value instead of jumping.
function countStars(target) {
  const from = shownStars ?? target;
  shownStars = target;
  const node = $("stars"), started = performance.now(), duration = from === target ? 0 : 2500;
  const step = (now) => {
    const t = duration ? Math.min(1, (now - started) / duration) : 1;
    node.textContent = fmt(Math.round(from + (target - from) * (1 - (1 - t) ** 3)));
    if (t < 1) requestAnimationFrame(step);
  };
  requestAnimationFrame(step);
  if (target > from) {
    const bump = $("bump");
    bump.textContent = `+${fmt(target - from)} ⭐`;
    bump.classList.remove("show");
    void bump.offsetWidth;
    bump.classList.add("show");
  }
  if (Math.floor(target / MILESTONE) > Math.floor(from / MILESTONE)) celebrate(Math.floor(target / MILESTONE) * MILESTONE);
}

function celebrate(stars) {
  const colors = ["#8b5cf6", "#ff4fa3", "#ff9a3c", "#ffd166", "#3987e5"];
  const confetti = Array.from({ length: 36 }, (_, index) => `<span class="confetti" style="left:${(index * 53) % 100}%;background:${colors[index % colors.length]};animation-duration:${3 + (index % 5) * 0.6}s;animation-delay:-${(index * 0.37).toFixed(2)}s"></span>`).join("");
  const node = $("takeover");
  node.className = "takeover leo-takeover show";
  node.innerHTML = `${confetti}<div class="takeover-text"><div class="takeover-kicker">Leonida just reached</div><div class="takeover-value">${fmt(stars)} ⭐</div><div class="takeover-note">GitHub stars in ${latest?.createdAt ? age(latest.createdAt) : "a few days"}</div></div>`;
  setTimeout(() => {
    node.className = "takeover";
    node.innerHTML = "";
  }, 12_000);
}

// The boot moment: when the status stops saying "no", again after a restart (the server keeps when it
// happened), then every 30 minutes for 12 hours so viewers arriving later see it too.
const BOOT_REPLAY_MS = 30 * 60_000, BOOT_HYPE_MS = 12 * 3_600_000;
let bootShownAt = 0;

function maybeCelebrateBoot(status) {
  if (!status.boot || status.boot.mark === "no") return;
  const since = Date.parse(latest?.bootSince ?? "");
  const fresh = Number.isNaN(since) || Date.now() - since < BOOT_HYPE_MS;
  if (fresh && Date.now() - bootShownAt >= BOOT_REPLAY_MS) {
    bootShownAt = Date.now();
    celebrateBoot(status.boot.note || status.headline);
  }
}

function celebrateBoot(note) {
  const colors = ["#8b5cf6", "#ff4fa3", "#ff9a3c", "#ffd166", "#0ca30c"];
  const confetti = Array.from({ length: 48 }, (_, index) => `<span class="confetti" style="left:${(index * 53) % 100}%;background:${colors[index % colors.length]};animation-duration:${3 + (index % 5) * 0.6}s;animation-delay:-${(index * 0.37).toFixed(2)}s"></span>`).join("");
  const node = $("takeover");
  node.className = "takeover leo-takeover show";
  node.innerHTML = `${confetti}<div class="takeover-text"><div class="takeover-kicker">It happened</div><div class="takeover-value leo-boot">GTA VI<br>BOOTS ON PC</div><div class="takeover-note">${escape(note || "Leonida's README now says it boots")}</div></div>`;
  setTimeout(() => {
    node.className = "takeover";
    node.innerHTML = "";
  }, 20_000);
}

function renderStars(leonida) {
  countStars(leonida.stars);
  const pace = leonida.starsLastHour ?? leonida.starsPerHour;
  $("pace").textContent = pace ? `+${fmt(pace)} / hour` : "";
  const today = leonida.starsToday ? ` · +${fmt(leonida.starsToday)} in 24 h` : "";
  $("since").textContent = leonida.createdAt ? `in ${age(leonida.createdAt)} since launch${today}` : "";
  const next = (Math.floor(leonida.stars / MILESTONE) + 1) * MILESTONE;
  $("goal-bar").style.width = `${(((leonida.stars % MILESTONE) / MILESTONE) * 100).toFixed(1)}%`;
  const minutes = pace ? Math.max(1, Math.round(((next - leonida.stars) / pace) * 60)) : null;
  const eta = minutes ? (minutes < 90 ? ` · about ${minutes} min` : ` · about ${Math.round(minutes / 60)} h`) : "";
  $("goal-text").textContent = `Next: ${fmt(next)} ⭐${eta}`;
}

// The header follows the README's boot row: red while the game does not boot, amber, then green. The moment
// it stops saying no, the whole screen says so.
function renderStatus(status) {
  if (!status) return;
  const mark = status.boot?.mark ?? "no";
  const icon = { done: "✅", partial: "🟡", no: "❌" }[mark] ?? "🟡";
  const fallback = mark === "no" ? "The game does not boot yet" : `Booting: ${status.boot?.note ?? "it boots"}`;
  $("headline").textContent = `${icon} ${status.headline || fallback}`;
  $("headline").className = `leo-headline ${mark}`;
  bootMark = mark;
  maybeCelebrateBoot(status);
  renderTicker();
  const rowIcon = { done: "✅", partial: "🟡", no: "❌" };
  $("rows").innerHTML = status.rows.map((row) => `<li class="${escape(row.mark)}"><span class="leo-mark">${rowIcon[row.mark] ?? "🟡"}</span><span class="leo-what">${escape(row.what)}</span><span class="leo-row-note">${escape(row.note)}</span></li>`).join("");
}

function renderPrs(list) {
  const known = seenPrs;
  seenPrs = new Set(list.map((pr) => pr.number));
  $("prs").innerHTML = list.slice(0, 6).map((pr) => `<li class="${known && !known.has(pr.number) ? "fresh" : ""}"><img src="${escape(pr.avatar)}&s=88" alt=""><div><div class="leo-pr-title">${escape(pr.title)}</div><div class="leo-pr-meta">#${pr.number} · ${escape(pr.author)} · ${ago(pr.mergedAt)}</div></div></li>`).join("");
}

// Once two hours of counts are kept (every 5 minutes), the last 24 hours, zoomed on their range. Before that,
// from the day it went public (0 stars, dashed: the shape in between is not known) to now.
const ZOOM_AFTER_MS = 2 * 3_600_000, ZOOM_SPAN_MS = 24 * 3_600_000;

function renderChart(history, createdAt, stars) {
  const svg = $("chart");
  let points = (history ?? []).map(([at, count]) => ({ at, stars: count }));
  if (stars && points[points.length - 1]?.stars !== stars) points.push({ at: Date.now(), stars });
  const zoom = points.length > 1 && points[points.length - 1].at - points[0].at >= ZOOM_AFTER_MS;
  if (zoom) points = points.filter((p) => p.at >= Date.now() - ZOOM_SPAN_MS);
  const start = Date.parse(createdAt ?? "");
  const origin = zoom || Number.isNaN(start) ? null : { at: start, stars: 0 };
  const width = svg.clientWidth || 1000, height = svg.clientHeight || 108;
  const all = origin ? [origin, ...points] : points;
  const minAt = all[0].at, maxAt = Math.max(all[all.length - 1].at, minAt + 1);
  const counts = all.map((p) => p.stars);
  const low = zoom ? Math.min(...counts) : 0, high = Math.max(...counts, low + 1);
  const x = (at) => ((at - minAt) / Math.max(1, maxAt - minAt)) * (width - 8) + 4;
  const y = (stars) => height - 6 - ((stars - low) / Math.max(1, high - low)) * (height - 16);
  const path = (list) => list.map((p, index) => `${index ? "L" : "M"}${x(p.at).toFixed(1)},${y(p.stars).toFixed(1)}`).join(" ");
  const known = path(points), first = points[0], last = points[points.length - 1];
  const guess = origin ? `<path d="${path([origin, first])}" fill="none" stroke="#ff4fa3" stroke-opacity="0.7" stroke-width="3" stroke-dasharray="8 8"/>` : "";
  svg.setAttribute("viewBox", `0 0 ${width} ${height}`);
  $("chart-title").textContent = zoom ? "Stars, the last 24 hours" : "Stars since it went public";
  svg.innerHTML = `<defs><linearGradient id="leo-line" x1="0" x2="1"><stop offset="0" stop-color="#8b5cf6"/><stop offset="0.5" stop-color="#ff4fa3"/><stop offset="1" stop-color="#ff9a3c"/></linearGradient><linearGradient id="leo-fill" x1="0" x2="0" y1="0" y2="1"><stop offset="0" stop-color="#ff4fa3" stop-opacity="0.35"/><stop offset="1" stop-color="#ff4fa3" stop-opacity="0"/></linearGradient></defs><path d="${path(all)} L${x(last.at).toFixed(1)},${height} L${x(minAt).toFixed(1)},${height} Z" fill="url(#leo-fill)"/>${guess}<path d="${known}" fill="none" stroke="url(#leo-line)" stroke-width="5" stroke-linejoin="round" stroke-linecap="round"/><circle cx="${x(last.at).toFixed(1)}" cy="${y(last.stars).toFixed(1)}" r="7" fill="#ff9a3c"/>`;
  $("chart-meta").textContent = origin ? `0 → ${fmt(last.stars)} ⭐ in ${age(createdAt)}` : `+${fmt(last.stars - first.stars)} ⭐ in ${age(new Date(first.at).toISOString())}`;
}

function render(state) {
  const leonida = state.leonida;
  if (!leonida) return;
  latest = leonida;
  renderStars(leonida);
  $("merged-today").textContent = fmt(leonida.mergedToday);
  $("merged-total").textContent = leonida.mergedTotal ? `${fmt(leonida.mergedTotal)} merged in all` : "";
  $("contributors").textContent = fmt(leonida.contributors);
  $("open-prs").textContent = fmt(leonida.openPrs);
  $("forks").textContent = leonida.forks ? `${fmt(leonida.forks)} forks` : "";
  $("feed-meta").textContent = leonida.mergedToday ? `${fmt(leonida.mergedToday)} today` : "";
  renderStatus(leonida.status);
  renderPrs(leonida.latestMerged ?? []);
  renderChart(leonida.starHistory, leonida.createdAt, leonida.stars);
}

function tickCta() {
  const cta = CTAS[Math.floor(Date.now() / CTA_MS) % CTAS.length];
  const box = $("cta-box");
  $("cta").textContent = typeof cta.text === "function" ? cta.text(bootMark && bootMark !== "no") : cta.text;
  $("cta-sub").textContent = cta.sub;
  if ($("cta-qr").getAttribute("src") !== cta.qr) $("cta-qr").src = cta.qr;
  box.classList.toggle("hangar", Boolean(cta.hangar));
  box.classList.remove("swap");
  void box.offsetWidth;
  box.classList.add("swap");
}

async function refresh() {
  try {
    const response = await fetch("/api/state", { cache: "no-store" });
    if (!response.ok) return;
    const state = await response.json();
    // A restarted dashboard is a deploy: reload, so the stream shows the change within a refresh.
    if (startedAt && state.startedAt !== startedAt) location.reload();
    startedAt = state.startedAt;
    render(state);
  } catch (error) {
    console.error("refresh", error);
  }
}

tickClock();
setInterval(tickClock, 5000);
renderTicker();
tickCta();
setInterval(tickCta, CTA_MS);
refresh().then(() => {
  if (params.get("demo") === "milestone") celebrate(Math.floor((latest?.stars ?? 23000) / MILESTONE) * MILESTONE);
  if (params.get("demo") === "boot") celebrateBoot("Boots to the title screen");
  if (params.get("cta")) {
    // ?cta=<n> pins one panel, to look at it.
    const pinned = CTAS[Number(params.get("cta")) % CTAS.length];
    CTAS.splice(0, CTAS.length, pinned);
    tickCta();
  }
});
setInterval(refresh, REFRESH_MS);
setTimeout(() => location.reload(), RELOAD_MS);
