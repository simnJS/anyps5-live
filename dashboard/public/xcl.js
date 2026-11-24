"use strict";

// XCL, live (ivall/xcl, Xbox Series games running natively on Windows), after Leonida was taken down from GitHub:
// the dashboard collects the repository every minute (src/leonida.rs, state key `leonida`); this page reads it
// every 10 seconds, counts the stars up, slides new merges in, and celebrates every 500 stars.
// ?demo=milestone plays the star celebration, ?demo=down the Leonida news.
const REFRESH_MS = 10_000, RELOAD_MS = 6 * 3_600_000, CTA_MS = 12_000, MILESTONE = 500;
// The panel under the "Just merged" feed, in turn. Each has its own QR code; HangarAI, which built this live,
// comes with a code to its site.
const LEONIDA_QR = { qr: "xcl-qr.svg", sub: "⭐ Scan to star it · github.com/ivall/xcl" };
const HANGAR_QR = { qr: "hangar-site-qr.svg", sub: "Coding agents in one window, free · hangarai.dev", hangar: true };
const CTAS = [
  { text: "⭐ Star XCL on GitHub", ...LEONIDA_QR },
  { text: "👍 Hit like if you want Xbox games on PC", ...LEONIDA_QR },
  { text: "🤖 This live was built with HangarAI", ...HANGAR_QR },
  { text: "🔔 Subscribe: every game that comes to life shows up here", ...LEONIDA_QR },
  { text: "🌴 GTA VI already shows its menus on PC with XCL", ...LEONIDA_QR },
  { text: "💬 Which Xbox game should run next? Tell the chat", ...LEONIDA_QR },
];
let bootMark = null;

// The warning band. "danger" lines are the scams; the boot line follows the README's boot row.
const TICKER_PX_PER_SECOND = 70;
const WARNINGS = () => [
  { text: "Leonida was taken down from GitHub: this live now follows XCL" },
  { text: "Never pay for a game .exe: it is a scam", danger: true },
  { text: "GTA VI runs to its menus on XCL, gameplay is not reached yet: every video of GTA VI gameplay on PC is fake", danger: true },
  { text: "XCL is free and open source: it runs games from your own copy, there is nothing to buy" },
  { text: "Never run files sent in comments, DMs, Discord or Telegram", danger: true },
  { text: "The only real source: github.com/ivall/xcl" },
  { text: "Unofficial fan tracker, not affiliated with Microsoft, Rockstar Games, Take-Two or the XCL maintainers" },
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
  const colors = ["#0b7a1a", "#2fd14a", "#b7f34a", "#ffd166", "#3987e5"];
  const confetti = Array.from({ length: 36 }, (_, index) => `<span class="confetti" style="left:${(index * 53) % 100}%;background:${colors[index % colors.length]};animation-duration:${3 + (index % 5) * 0.6}s;animation-delay:-${(index * 0.37).toFixed(2)}s"></span>`).join("");
  const node = $("takeover");
  node.className = "takeover leo-takeover show";
  node.innerHTML = `${confetti}<div class="takeover-text"><div class="takeover-kicker">XCL just reached</div><div class="takeover-value">${fmt(stars)} ⭐</div><div class="takeover-note">GitHub stars in ${latest?.createdAt ? age(latest.createdAt) : "a few days"}</div></div>`;
  setTimeout(() => {
    node.className = "takeover";
    node.innerHTML = "";
  }, 12_000);
}

// The news, full screen: when the page opens, then every 30 minutes until the date below, for viewers who
// arrive later and still look for Leonida.
const NEWS_REPLAY_MS = 30 * 60_000, NEWS_UNTIL = Date.parse("2026-11-25T12:00:00Z");
let newsShownAt = 0;

function maybeShowNews() {
  if (Date.now() > NEWS_UNTIL || Date.now() - newsShownAt < NEWS_REPLAY_MS) return;
  newsShownAt = Date.now();
  showNews();
}

function showNews() {
  const node = $("takeover");
  node.className = "takeover leo-takeover xcl-news show";
  node.innerHTML = `<div class="takeover-text"><div class="takeover-kicker">Breaking</div><div class="takeover-value leo-boot">LEONIDA WAS<br>TAKEN DOWN</div><div class="takeover-note">It is gone from GitHub. This live now follows <strong>XCL</strong>: Xbox Series games running natively on PC</div></div>`;
  setTimeout(() => {
    node.className = "takeover";
    node.innerHTML = "";
  }, 18_000);
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

// XCL's README calls it "a newer retail game": the stream names it GTA VI (its shader pack is GTA VI's), puts it
// first in gold, and gives it the header.
const GTA_ROW = /newer retail game/i;

// Whole clauses only: the server cuts a long README note at 90 characters, mid-word.
function wholeClauses(note) {
  const parts = String(note ?? "").split(", ");
  if (note.length >= 89 && parts.length > 1) parts.pop();
  const text = parts.join(", ");
  return text.charAt(0).toUpperCase() + text.slice(1);
}

function renderStatus(status) {
  if (!status) return;
  const gta = status.rows.find((row) => GTA_ROW.test(row.what));
  const others = status.rows.filter((row) => row !== gta);
  const playable = others.filter((row) => /fully playable/i.test(row.note)).length;
  if (gta) {
    const where = gta.mark === "done" ? "✅ GTA VI runs on PC with XCL" : "🌴 GTA VI on PC with XCL: menus showing";
    $("headline").textContent = `${where}${gta.limits ? `, ${gta.limits}` : ""}`;
    $("headline").className = "leo-headline gta";
  } else {
    $("headline").textContent = `✅ ${status.headline || "Retail games run"} on PC${playable ? ` · ${playable} games fully playable` : ""}`;
    $("headline").className = "leo-headline done";
  }
  renderTicker();
  const rowIcon = { done: "✅", partial: "🟡", no: "❌" };
  const row = (item) => `<li class="${escape(item.mark)}"><span class="leo-mark">${rowIcon[item.mark] ?? "🟡"}</span><span class="leo-what">${escape(item.what)}</span><span class="leo-row-note">${escape(item.note)}</span></li>`;
  const gtaRow = gta ? `<li class="gta"><span class="leo-mark">🌴</span><span class="leo-what">GTA VI</span><span class="leo-row-note">${escape(wholeClauses(gta.note))}${gta.limits ? ` · <em>${escape(gta.limits)}</em>` : ""}</span></li>` : "";
  $("rows").innerHTML = gtaRow + others.slice(0, gta ? 10 : 13).map(row).join("");
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
  const guess = origin ? `<path d="${path([origin, first])}" fill="none" stroke="#2fd14a" stroke-opacity="0.7" stroke-width="3" stroke-dasharray="8 8"/>` : "";
  svg.setAttribute("viewBox", `0 0 ${width} ${height}`);
  $("chart-title").textContent = zoom ? "Stars, the last 24 hours" : "Stars since it went public";
  svg.innerHTML = `<defs><linearGradient id="leo-line" x1="0" x2="1"><stop offset="0" stop-color="#0b7a1a"/><stop offset="0.5" stop-color="#2fd14a"/><stop offset="1" stop-color="#b7f34a"/></linearGradient><linearGradient id="leo-fill" x1="0" x2="0" y1="0" y2="1"><stop offset="0" stop-color="#2fd14a" stop-opacity="0.35"/><stop offset="1" stop-color="#2fd14a" stop-opacity="0"/></linearGradient></defs><path d="${path(all)} L${x(last.at).toFixed(1)},${height} L${x(minAt).toFixed(1)},${height} Z" fill="url(#leo-fill)"/>${guess}<path d="${known}" fill="none" stroke="url(#leo-line)" stroke-width="5" stroke-linejoin="round" stroke-linecap="round"/><circle cx="${x(last.at).toFixed(1)}" cy="${y(last.stars).toFixed(1)}" r="7" fill="#b7f34a"/>`;
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
  if (params.get("demo") === "down") showNews();
  else maybeShowNews();
  if (params.get("cta")) {
    // ?cta=<n> pins one panel, to look at it.
    const pinned = CTAS[Number(params.get("cta")) % CTAS.length];
    CTAS.splice(0, CTAS.length, pinned);
    tickCta();
  }
});
setInterval(refresh, REFRESH_MS);
setTimeout(() => location.reload(), RELOAD_MS);
