"use strict";

const REFRESH_MS = 15_000;
const RELOAD_MS = 6 * 3_600_000;
const KIND_COLORS = {
  discovery: "var(--series-1)",
  jump: "var(--series-2)",
  percent: "var(--series-3)",
  milestone: "var(--series-4)",
  complete: "var(--series-5)",
  release: "var(--series-7)",
  library: "var(--series-8)",
};

const numberFormat = new Intl.NumberFormat("en-US");
const fmt = (value) => (value === null || value === undefined ? "—" : numberFormat.format(value));
const pct = (value) => `${value.toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}%`;
const signed = (value) => `${value > 0 ? "+" : value < 0 ? "−" : ""}${fmt(Math.abs(value))}`;
const $ = (id) => document.getElementById(id);
const escape = (text) => String(text ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const avatar = (url, size) => `${url}${url.includes("?") ? "&" : "?"}s=${size}`;
const shortDate = (iso) => new Date(iso).toLocaleDateString("en-US", { day: "numeric", month: "short", timeZone: "UTC" });

function ago(iso) {
  const seconds = Math.max(0, (Date.now() - Date.parse(iso)) / 1000);
  if (seconds < 60) return "just now";
  if (seconds < 3600) return `${Math.floor(seconds / 60)} min ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)} h ago`;
  return `${Math.floor(seconds / 86400)} d ago`;
}

function tickClock() {
  $("clock").textContent = new Date().toLocaleTimeString("en-GB", { timeZone: "UTC", hour: "2-digit", minute: "2-digit", hour12: false });
}

function renderKpis(state) {
  const progress = state.progress;
  if (progress) {
    countTo($("hero-done"), progress.done);
    $("hero-count").textContent = `${pct(progress.percent)} of ${fmt(progress.total)} found so far`;
    const parts = [];
    if (progress.day) parts.push(`<span class="up">${signed(progress.day.done)}</span> in 24h`);
    if (progress.week) parts.push(`<span class="up">${signed(progress.week.done)}</span> in 7 days`);
    $("hero-delta").innerHTML = parts.join(" · ") || "&nbsp;";
    $("hero-meter").style.width = `${Math.min(100, progress.percent)}%`;
  }
  const github = state.github;
  if (github) {
    $("kpi-merged24").textContent = fmt(github.merged24h);
    $("kpi-merged7").textContent = `${fmt(github.merged7d)} this week`;
    $("kpi-open-prs").textContent = fmt(github.openPrs);
    $("kpi-stars").textContent = fmt(github.stars);
    $("kpi-release").textContent = github.release ? `latest release ${github.release.tag}` : "";
  }
  if (state.games) {
    $("kpi-ingame").textContent = fmt(state.games.inGame);
    $("kpi-games-note").textContent = `of ${fmt(state.games.tracked)} reported`;
  }
}

// Rolls a number up to its new value, so a change on screen is seen and not just swapped in.
function countTo(node, target) {
  const from = Number(node.dataset.value);
  node.dataset.value = target;
  if (!Number.isFinite(from) || from === target || !node.dataset.counted) {
    node.dataset.counted = "1";
    node.textContent = fmt(target);
    return;
  }
  const started = performance.now();
  const step = (now) => {
    const progress = Math.min(1, (now - started) / 2000);
    node.textContent = fmt(Math.round(from + (target - from) * progress));
    if (progress < 1) requestAnimationFrame(step);
  };
  requestAnimationFrame(step);
}

function niceMax(value) {
  const step = 10 ** Math.floor(Math.log10(value));
  return Math.ceil(value / step) * step;
}

function renderChart(history) {
  const svg = $("chart");
  const width = svg.clientWidth, height = svg.clientHeight;
  if (!history.length || !width || !height) return;
  svg.setAttribute("viewBox", `0 0 ${width} ${height}`);
  const left = 58, right = 118, top = 10, bottom = 30;
  const times = history.map((point) => Date.parse(`${point.date}T12:00:00Z`));
  const first = times[0], last = times[times.length - 1];
  const yMax = niceMax(Math.max(...history.map((point) => point.total)) * 1.05);
  const x = (time) => left + ((time - first) / Math.max(1, last - first)) * (width - left - right);
  const y = (value) => top + (1 - value / yMax) * (height - top - bottom);
  const path = (key) => history.map((point, index) => `${index ? "L" : "M"}${x(times[index]).toFixed(1)},${y(point[key]).toFixed(1)}`).join("");

  const parts = [];
  for (let index = 0; index <= 3; index += 1) {
    const value = (yMax / 3) * index;
    const py = y(value);
    parts.push(`<line class="${index ? "grid" : "axis"}" x1="${left}" x2="${width - right + 8}" y1="${py}" y2="${py}"/>`);
    parts.push(`<text class="tick" x="${left - 10}" y="${py + 5}" text-anchor="end">${value >= 1000 ? `${fmt(Math.round(value / 1000))}k` : fmt(value)}</text>`);
  }
  for (let time = first; time <= last; time += 14 * 86_400_000) {
    parts.push(`<text class="tick" x="${x(time)}" y="${height - 6}" text-anchor="middle">${shortDate(new Date(time).toISOString())}</text>`);
  }
  parts.push(`<path class="area" d="${path("done")}L${x(last)},${y(0)}L${x(first)},${y(0)}Z" fill="var(--series-1)"/>`);
  parts.push(`<path class="line" d="${path("total")}" stroke="var(--series-2)"/>`);
  parts.push(`<path class="line" d="${path("done")}" stroke="var(--series-1)"/>`);

  const end = history[history.length - 1];
  const doneY = y(end.done), totalY = y(end.total);
  const gap = Math.abs(doneY - totalY) < 44 ? 44 : 0;
  parts.push(`<text class="end-label" x="${x(last) + 14}" y="${totalY + 4}">${fmt(end.total)}</text>`);
  parts.push(`<text class="end-note" x="${x(last) + 14}" y="${totalY + 22}">declared</text>`);
  parts.push(`<text class="end-label" x="${x(last) + 14}" y="${doneY + 4 + gap}">${fmt(end.done)}</text>`);
  parts.push(`<text class="end-note" x="${x(last) + 14}" y="${doneY + 22 + gap}">implemented</text>`);
  svg.innerHTML = parts.join("");
}

function renderPrs(github) {
  if (!github) return;
  renderPrList($("prs"), github.mergedPrs, (pr) => pr.mergedAt);
  renderPrList($("opened"), github.openedPrs ?? [], (pr) => pr.createdAt);
}

function renderPrList(list, prs, timeOf) {
  const known = new Set([...list.querySelectorAll(".pr")].map((node) => node.dataset.number));
  const html = prs
    .slice(0, 3)
    .map((pr) => {
      const fresh = known.size && !known.has(String(pr.number)) ? " fresh" : "";
      const implemented = pr.report?.implemented ?? 0, declared = pr.report?.declared ?? 0;
      const pill = implemented > 0
        ? `<span class="pill">+${fmt(implemented)} function${implemented > 1 ? "s" : ""}</span>`
        : declared > 0 ? `<span class="pill declared">+${fmt(declared)} declared</span>` : "";
      return `<li class="pr${fresh}" data-number="${pr.number}"><img class="avatar" src="${avatar(pr.avatar, 88)}" alt=""><div class="pr-text"><div class="pr-title">${escape(pr.title)}</div><div class="pr-meta">#${pr.number} · ${escape(pr.author)} · ${ago(timeOf(pr))}</div></div>${pill}</li>`;
    })
    .join("");
  // Rewriting an unchanged list makes its avatars flicker on air.
  if (list.dataset.html !== html) {
    list.dataset.html = html;
    list.innerHTML = html;
  }
}

// "Exports discovered" lines say the total grew, which is noise next to what got done: they only fill up a quiet list.
const eventTitle = (title) => title.replace(/^(lib[\w.]+): /, (_, name) => `${libraryLabel(name)}: `);

function renderEvents(events) {
  const done = events.filter((event) => event.kind !== "discovery");
  const found = events.filter((event) => event.kind === "discovery" && !event.title.includes("exports discovered"));
  const list = [...done, ...found].slice(0, 5);
  $("events").innerHTML = list.length
    ? list
        .map((event) => `<li class="event"><span class="event-dot" style="background:${KIND_COLORS[event.kind] ?? "var(--series-1)"}"></span><span class="event-title">${escape(eventTitle(event.title))}</span><span class="event-date">${shortDate(event.time)}</span></li>`)
        .join("")
    : `<li class="empty">Nothing yet.</li>`;
}

function renderGames(games) {
  if (!games) return;
  $("games-meta").textContent = `${fmt(games.active24h)} active today`;
  const max = Math.max(1, ...games.stages.map((stage) => stage.count));
  $("stages").innerHTML = games.stages
    .map((stage) => `<div class="stage"><span class="label">${escape(stage.name)}</span><span class="bar"><span style="width:${((stage.count / max) * 100).toFixed(1)}%"></span></span><span class="count">${fmt(stage.count)}</span></div>`)
    .join("");
}

let mapKey = "";
function renderMap(libraries) {
  if (!libraries) return;
  $("map-meta").textContent = `${fmt(libraries.count)} libraries · ${fmt(libraries.complete)} complete`;
  const canvas = $("map");
  const key = `${canvas.clientWidth}x${canvas.clientHeight}|${libraries.all.map((library) => `${library.name}:${library.done}/${library.total}`).join(",")}`;
  if (key === mapKey) return;
  mapKey = key;
  drawLibraryMap(canvas, libraries.all);
}

const MILESTONE_STEP = 1000;
let latest = null;

// GTA VI on AnyPS5: its stage and the firmware figures come from the dashboard (src/gta6.rs).
const THIS_GAME = /grand theft auto\s*(vi|6)\b|\bgta\s*(vi|6)\b/i;

function renderGtaChip(gta) {
  const node = $("gta-chip");
  node.hidden = !gta;
  if (!gta) return;
  const firmware = gta.stage === "released" ? ` · needs firmware <strong>${escape(gta.firmware.required)}</strong> · jailbreak ${escape(gta.firmware.jailbreak)}` : "";
  node.innerHTML = `🌴 GTA VI: <strong class="gta-chip-stage">${escape(gta.label)}</strong>${firmware}`;
}

function sinceText(iso) {
  const seconds = Math.max(0, Math.floor((Date.now() - Date.parse(iso)) / 1000));
  const pad = (value) => String(value).padStart(2, "0");
  return `${Math.floor(seconds / 86400)}d ${pad(Math.floor(seconds / 3600) % 24)}:${pad(Math.floor(seconds / 60) % 60)}:${pad(seconds % 60)}`;
}

function tickGtaSince() {
  const node = $("gta6-since");
  if (node && latest?.gta6) node.textContent = sinceText(latest.gta6.releasedAt);
}

function renderGta6Scene() {
  const gta = latest?.gta6;
  if (!gta) return false;
  const steps = gta.steps.map((step, index) => `<div class="gta6-step ${index <= gta.step ? "passed" : index === gta.step + 1 ? "current" : ""}">${escape(step)}</div>`).join("");
  const firmware = gta.stage === "released" ? `<div class="gta6-firmware">
      <div><div class="gta6-fw">${escape(gta.firmware.required)}</div><div class="gta6-fw-label">PS5 firmware GTA VI needs</div></div>
      <div class="gta6-vs">vs</div>
      <div class="jailbreak"><div class="gta6-fw">${escape(gta.firmware.jailbreak)}</div><div class="gta6-fw-label">highest public jailbreak</div></div>
    </div>` : "";
  const thread = latest.games?.rockstar?.find((game) => THIS_GAME.test(game.name));
  const report = thread && gta.reportsCount ? `First AnyPS5 reports: ${escape(thread.stage ?? "being tested")} · ${fmt(thread.replies)} replies.` : escape(gta.detail);
  const rockstar = (latest.games?.rockstar ?? []).filter((game) => !THIS_GAME.test(game.name)).slice(0, 5)
    .map((game) => `<li><span class="gta6-game-name">${escape(game.name)}</span><span class="gta6-game-stage ${game.stage === "In game" ? "good" : ""}">${escape(game.stage ?? "Being tested")}</span></li>`).join("");
  $("scene-gta6").innerHTML = `<div class="scene-body gta6-scene">
    <div class="gta6-main">
      <div><div class="scene-tag">GTA VI on AnyPS5</div><h2 class="scene-title">${escape(gta.title)}</h2></div>
      ${firmware}
      <div class="gta6-steps">${steps}</div>
      <div class="scene-note">${report}</div>
      ${latest.chat?.enabled ? `<div class="chat-hint">💬 Type <strong>!gta</strong> in the chat for its status at any time</div>` : ""}
    </div>
    <aside class="gta6-side">
      <div class="gta6-since-box"><div class="gta6-side-title">Out on PS5 for</div><div class="gta6-since" id="gta6-since">${sinceText(gta.releasedAt)}</div></div>
      ${rockstar ? `<div class="gta6-side-title">Rockstar games on AnyPS5 today</div><ol class="gta6-games">${rockstar}</ol>` : ""}
    </aside>
  </div>`;
  return true;
}

// Scenes
let spotlightIndex = Math.floor(Date.now() / 60_000) % 1000; // A page reload does not start over with the same game.
const STAGE_ORDER = ["Does not convert", "Stops before first frame", "Logos or loading", "Menu", "In game"];

function coverHtml(game) {
  const name = `<div class="cover-name">${escape(game.name)}</div>`;
  if (!game.cover) return `<div class="cover">${name}</div>`;
  return `<div class="cover">${name}<img src="${escape(game.cover)}" alt="" onerror="this.remove()"></div>`;
}

// Games reaching gameplay, best known first. Every other turn goes to the top few, so the games
// viewers are likely to know come back far more often than the rest.
const SPOTLIGHT_TOP = 6;

function pickSpotlight(pool, turn) {
  const top = pool.slice(0, SPOTLIGHT_TOP), rest = pool.slice(SPOTLIGHT_TOP);
  if (!rest.length || turn % 2 === 0) return top[Math.floor(turn / 2) % top.length];
  return rest[Math.floor(turn / 2) % rest.length];
}

function renderSpotlight() {
  const pool = latest?.games?.highlights ?? [];
  if (!pool.length) return false;
  const game = pickSpotlight(pool, spotlightIndex);
  spotlightIndex += 1;
  const reached = STAGE_ORDER.indexOf(game.stage);
  const steps = STAGE_ORDER.map((stage, index) => `<div class="track-step ${index === reached ? "current" : index < reached ? "passed" : ""}">${escape(stage)}</div>`).join("");
  const others = pool.filter((other) => other !== game).slice(0, 4)
    .map((other) => `<div class="other"><div class="other-name">${escape(other.name)}</div><div class="other-stage">${escape(other.stage)} · ${ago(other.updatedAt)}</div></div>`).join("");
  const replies = game.replies ? ` · ${fmt(game.replies)} ${game.replies > 1 ? "replies" : "reply"}` : "";
  $("scene-games").innerHTML = `<div class="scene-body">
    <div class="spotlight-main">${coverHtml(game)}
      <div class="spotlight-text">
        <div><div class="scene-tag">Game spotlight</div><h2 class="scene-title">${escape(game.name)}</h2></div>
        <div class="track">${steps}</div>
        <div class="spotlight-meta">Report updated ${ago(game.updatedAt)}${replies}</div>
        ${latest?.chat?.enabled ? `<div class="chat-hint">💬 Type <strong>!game</strong> and a name to check your game</div>` : ""}
      </div>
    </div>
    <div class="spotlight-others">${others}</div>
  </div>`;
  return true;
}

// Measured fps chart. Its figures and texts live in perf.json, the only file to edit: each group is a panel,
// each bar has a kind, "start" (where it began), "shipped" or "test" (measured, not shipped yet).
let perf = null;
const PERF_REFRESH_MS = 5 * 60_000;

async function refreshPerf() {
  try {
    const response = await fetch("perf.json", { cache: "no-store" });
    if (response.ok) perf = await response.json();
  } catch (error) {
    console.error("perf", error);
  }
}

const oneDecimal = (value) => value.toLocaleString("en-US", { minimumFractionDigits: 1, maximumFractionDigits: 1 });

// The gain over the group's start: what shipped as a percentage, what is still in test as a factor.
function perfGains(bars) {
  const start = bars.find((bar) => bar.kind === "start")?.fps;
  if (!start) return "";
  const best = (kind) => Math.max(0, ...bars.filter((bar) => bar.kind === kind).map((bar) => bar.fps));
  const parts = [];
  if (best("shipped")) parts.push(`<span class="perf-gain">+${Math.round((best("shipped") / start - 1) * 100)} %</span>`);
  if (best("test")) parts.push(`<span class="perf-gain test">×${oneDecimal(best("test") / start)} in test</span>`);
  return parts.join("");
}

function perfPanel(group, max, target) {
  const ticks = [];
  for (let value = 0; value <= max; value += 20) ticks.push(`<div class="perf-tick" style="bottom:${(value / max) * 100}%"><span>${value}</span></div>`);
  const line = target ? `<div class="perf-target" style="bottom:${(target / max) * 100}%"><span>${target} fps</span></div>` : "";
  const bars = group.bars.map((bar, index) => {
    const height = (Math.min(bar.fps, max) / max) * 100;
    return `<div class="perf-col"><div class="perf-bar ${escape(bar.kind)}" style="height:${height}%;animation-delay:${index * 0.12}s"></div><div class="perf-value" style="bottom:${height}%;animation-delay:${0.5 + index * 0.12}s"><span>${oneDecimal(bar.fps)}</span></div></div>`;
  }).join("");
  const labels = group.bars.map((bar) => `<div class="perf-label"><strong>${escape(bar.label)}</strong>${bar.detail ? `<span>${escape(bar.detail)}</span>` : ""}${bar.kind === "test" ? `<em>in test, not shipped</em>` : ""}</div>`).join("");
  return `<section class="perf-panel" style="grid-column:span ${group.bars.length}">
    <header class="perf-panel-head"><h3>${escape(group.name)}</h3>${perfGains(group.bars)}</header>
    <div class="perf-plot">${ticks.join("")}${line}<div class="perf-bars">${bars}</div></div>
    <div class="perf-labels">${labels}</div>
  </section>`;
}

function renderPerf() {
  if (!perf?.groups?.length) return false;
  const max = perf.max ?? 100;
  const columns = perf.groups.reduce((sum, group) => sum + group.bars.length, 0);
  const kinds = new Set(perf.groups.flatMap((group) => group.bars.map((bar) => bar.kind)));
  const legend = [["start", "Starting point"], ["shipped", "Shipped"], ["test", "In test"]].filter(([kind]) => kinds.has(kind))
    .map(([kind, name]) => `<span class="perf-key"><span class="perf-swatch ${kind}"></span>${name}</span>`).join("");
  $("scene-perf").innerHTML = `<div class="scene-body perf-scene">
    <div class="perf-head">
      <div><div class="scene-tag">${escape(perf.tag)}</div><h2 class="scene-title">${escape(perf.title)}</h2><div class="perf-setup">${escape(perf.setup)}</div></div>
      <div class="perf-legend">${legend}</div>
    </div>
    <div class="perf-panels" style="grid-template-columns:repeat(${columns}, 1fr)">${perf.groups.map((group) => perfPanel(group, max, perf.target)).join("")}</div>
    ${perf.notes?.length ? `<ul class="perf-notes">${perf.notes.map((note) => `<li>${escape(note)}</li>`).join("")}</ul>` : ""}
  </div>`;
  return true;
}

// HangarAI, the open-source app the AnyKyty fork and this live were built with. Every text of its scene and card is here.
const PROMO = {
  name: "HangarAI",
  site: "hangarai.dev",
  discord: "discord.gg/hangarai",
  tag: "Made with HangarAI",
  title: "The AnyKyty fork and this live were built with HangarAI",
  pitch: "A team of coding agents in one native window. Claude Code and Codex: an orchestrator plans, workers build in git worktrees, Hangar checks and lands.",
  facts: ["Free and open source", "Native Rust app, no Electron", "Windows · macOS · Linux"],
  // The card is one line each: the kicker reads into the title.
  cardKicker: "AnyKyty and this live were built with",
};

function renderHangar() {
  $("scene-hangar").innerHTML = `<div class="scene-body hangar-scene">
    <div class="hangar-main">
      <div class="hangar-brand"><img src="hangar-logo.svg" alt=""><span>${escape(PROMO.name)}</span><code>${escape(PROMO.site)}</code></div>
      <div><div class="scene-tag">${escape(PROMO.tag)}</div><h2 class="scene-title">${escape(PROMO.title)}</h2></div>
      <p class="hangar-pitch">${escape(PROMO.pitch)}</p>
      <ul class="hangar-facts">${PROMO.facts.map((fact) => `<li>${escape(fact)}</li>`).join("")}</ul>
      <div class="hangar-join">
        <img class="hangar-qr" src="hangar-discord-qr.svg" alt="">
        <div><div class="hangar-join-title">Join the Discord</div><div class="hangar-join-url">${escape(PROMO.discord)}</div><div class="hangar-join-site">Download it free at <strong>${escape(PROMO.site)}</strong></div></div>
      </div>
    </div>
    <div class="hangar-shot"><img src="hangar-app.webp" alt=""></div>
  </div>`;
  return true;
}

function hangarCta() {
  return { kind: "hangar", kicker: PROMO.cardKicker, title: `${PROMO.name} · ${PROMO.site}`, meta: `Free and open source · ${PROMO.discord}`, image: "hangar-logo.svg" };
}

function renderContributors() {
  const github = latest?.github;
  if (!github?.topAuthors?.length) return false;
  $("scene-contributors").innerHTML = `<div class="scene-body">
    <div><div class="scene-tag">Who is building it</div><h2 class="scene-title">Most merged PRs today</h2></div>
    <div class="authors">${github.topAuthors.map((author) => `<div class="author"><img src="${escape(avatar(author.avatar, 220))}" alt=""><div class="author-text"><div class="author-name">${escape(author.author)}</div><div class="author-count">${fmt(author.merged)} PR${author.merged > 1 ? "s" : ""}</div></div></div>`).join("")}</div>
    <div class="scene-note">${fmt(github.merged24h)} PRs merged in the last 24 hours</div>
  </div>`;
  return true;
}

function renderMovers() {
  const libraries = latest?.libraries;
  const movers = libraries?.movers?.slice(0, 6) ?? [];
  if (!movers.length) return false;
  $("scene-movers").innerHTML = `<div class="scene-body">
    <div><div class="scene-tag">Moving fast</div><h2 class="scene-title">Biggest gains since ${libraries.moversSince ? shortDate(libraries.moversSince) : "yesterday"}</h2></div>
    <div class="movers">${movers.map((mover) => `<div class="mover"><div><div class="mover-name">${escape(libraryLabel(mover.name))}</div><div class="mover-lib">${escape(mover.name)} · ${fmt(mover.done)} / ${fmt(mover.total)}</div></div><div class="mover-bar"><span style="width:${mover.total ? ((mover.done / mover.total) * 100).toFixed(1) : 0}%"></span></div><div class="mover-gain">${signed(mover.gained)}</div></div>`).join("")}</div>
  </div>`;
  return true;
}

// How long each scene stays on screen, in seconds.
const DASHBOARD_SECONDS = 30, GTA_SECONDS = 25, PERF_SECONDS = 20, HANGAR_SECONDS = 15, SCENE_SECONDS = 15;
const SCENES = {
  dashboard: { node: "scene-dashboard", title: "Live dashboard", render: () => true, seconds: DASHBOARD_SECONDS },
  games: { node: "scene-games", title: "Game spotlight", render: renderSpotlight, seconds: SCENE_SECONDS },
  contributors: { node: "scene-contributors", title: "Top contributors", render: renderContributors, seconds: SCENE_SECONDS },
  perf: { node: "scene-perf", title: "GTA V fps", render: renderPerf, seconds: PERF_SECONDS },
  hangar: { node: "scene-hangar", title: "Made with HangarAI", render: renderHangar, seconds: HANGAR_SECONDS },
  movers: { node: "scene-movers", title: "Biggest gains", render: renderMovers, seconds: SCENE_SECONDS },
  gta6: { node: "scene-gta6", title: "GTA VI", render: renderGta6Scene, seconds: GTA_SECONDS },
};
// Most viewers come for GTA VI: it fills every other turn, about half the time. The game spotlight and the top
// contributors are out of the rotation for now and can still be pinned with ?scene=games or ?scene=contributors.
// HangarAI follows the GTA V fps chart, since that work was built with it.
const ROTATION = ["dashboard", "gta6", "perf", "hangar", "gta6", "dashboard", "gta6", "movers", "gta6"];
const params = new URLSearchParams(location.search);
const pinnedScene = SCENES[params.get("scene")] ? params.get("scene") : null;
const TRANSITION_MS = 500, PREPARE_SECONDS = 3;
let rotationIndex = 0;
let sceneEnds = 0;
let shownScene = "dashboard";
let prepared = null;

function showScene(name) {
  const previous = $(SCENES[shownScene].node), node = $(SCENES[name].node);
  node.classList.add("entering");
  shownScene = name;
  sceneEnds = Date.now() + SCENES[name].seconds * 1000;
  setTimeout(() => {
    previous.classList.remove("active");
    node.classList.add("active");
    node.classList.remove("entering");
  }, TRANSITION_MS);
}

function nextSceneName() {
  for (let offset = 1; offset <= ROTATION.length; offset += 1) {
    const index = (rotationIndex + offset) % ROTATION.length;
    if (ROTATION[index] !== shownScene) return { name: ROTATION[index], index };
  }
  return { name: "dashboard", index: 0 };
}

// The scene is filled and its picture decoded seconds ahead, so the slide itself has nothing else to do.
// A scene whose data is not there yet is skipped.
function prepareNext() {
  for (let tries = 0; tries < ROTATION.length; tries += 1) {
    const next = nextSceneName();
    if (SCENES[next.name].render()) {
      $(SCENES[next.name].node).querySelectorAll("img").forEach((img) => img.decode().catch(() => {}));
      return next;
    }
    rotationIndex = next.index;
  }
  return null;
}

// The scenes wait while a gameplay clip plays (gameplay.js), and the one under it gets its full time back after.
document.addEventListener("gameplay:end", () => {
  sceneEnds = Date.now() + SCENES[shownScene].seconds * 1000;
  prepared = null;
});
const GAMEPLAY_WARNING_MS = 60_000;
let gameplayWarned = 0;

// The header counts down to a clip a minute ahead, and a card announces it.
function gameplayCountdown() {
  const at = window.Gameplay?.next();
  const left = at ? at - Date.now() : Infinity;
  if (left > GAMEPLAY_WARNING_MS) return false;
  if (gameplayWarned !== at) {
    gameplayWarned = at;
    pushAlert({ kind: "cta gameplay-soon", kicker: "Coming up in 1 minute", title: window.Gameplay.title(), meta: "Real gameplay, recorded on AnyKyty", icon: "🎮" });
  }
  const seconds = Math.max(0, Math.ceil(left / 1000));
  $("next-up").innerHTML = `Next: <strong>Gameplay</strong> in ${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
  return true;
}

function tickScenes() {
  if (window.Gameplay?.playing()) return;
  if (pinnedScene) {
    if (shownScene !== pinnedScene && latest && SCENES[pinnedScene].render()) showScene(pinnedScene);
    return;
  }
  if (!sceneEnds) sceneEnds = Date.now() + DASHBOARD_SECONDS * 1000;
  let left = Math.ceil((sceneEnds - Date.now()) / 1000);
  if (left <= PREPARE_SECONDS && !prepared && latest) prepared = prepareNext();
  if (left <= 0) {
    if (prepared) {
      rotationIndex = prepared.index;
      showScene(prepared.name);
      prepared = null;
    } else {
      sceneEnds = Date.now() + SCENES[shownScene].seconds * 1000;
    }
    left = SCENES[shownScene].seconds;
  }
  if (gameplayCountdown()) return;
  const next = SCENES[prepared?.name ?? nextSceneName().name];
  $("next-up").innerHTML = `Next: <strong>${next.title}</strong> in ${Math.floor(left / 60)}:${String(left % 60).padStart(2, "0")}`;
}

// Alerts: new merged PRs, new game reports and finished libraries slide in one at a time.
const ALERT_MS = 9000, ALERT_GAP_MS = 1500, ALERT_QUEUE = 6;
const alertQueue = [];
const seen = { prs: null, games: null, events: null, chat: null, done: null, likes: null };
let alertBusy = false;
let alertTimer = null;
let shownAlert = null;

// A viewer waiting for an answer goes before the automatic cards, and is the last to be dropped.
function pushAlert(alert) {
  const at = alert.priority ? alertQueue.findIndex((queued) => !queued.priority) : -1;
  if (at >= 0) alertQueue.splice(at, 0, alert);
  else alertQueue.push(alert);
  while (alertQueue.length > ALERT_QUEUE) {
    const drop = alertQueue.findLastIndex((queued) => !queued.priority);
    alertQueue.splice(drop >= 0 ? drop : 0, 1);
  }
  // A viewer waiting for an answer does not sit through an automatic card.
  if (alert.priority && shownAlert && !shownAlert.priority) {
    clearTimeout(alertTimer);
    endAlert(100);
    return;
  }
  playAlerts();
}

function playAlerts() {
  if (alertBusy || !alertQueue.length || $("takeover").classList.contains("show")) return;
  alertBusy = true;
  const alert = alertQueue.shift();
  shownAlert = alert;
  const image = alert.image ? `<img src="${escape(alert.image)}" alt="">` : `<div class="alert-icon">${alert.icon ?? "★"}</div>`;
  $("alerts").innerHTML = `<div class="alert ${alert.kind ?? ""}">${image}<div class="alert-text"><div class="alert-kicker">${escape(alert.kicker)}</div><div class="alert-title">${escape(alert.title)}</div>${alert.meta ? `<div class="alert-meta">${escape(alert.meta)}</div>` : ""}${typeof alert.progress === "number" ? `<div class="alert-bar"><span style="width:${Math.min(100, alert.progress).toFixed(1)}%"></span></div>` : ""}</div>${alert.pill ? `<span class="pill">${escape(alert.pill)}</span>` : "<span></span>"}</div>`;
  alertTimer = setTimeout(endAlert, ALERT_MS, ALERT_GAP_MS);
}

function endAlert(gap) {
  $("alerts").innerHTML = "";
  shownAlert = null;
  setTimeout(() => {
    alertBusy = false;
    playAlerts();
  }, gap);
}

function celebrate(kicker, value, note) {
  const colors = ["var(--series-1)", "var(--series-2)", "var(--series-3)", "var(--series-4)", "var(--series-5)"];
  const confetti = Array.from({ length: 36 }, (_, index) => `<span class="confetti" style="left:${(index * 53) % 100}%;background:${colors[index % colors.length]};animation-duration:${3 + (index % 5) * 0.6}s;animation-delay:-${(index * 0.37).toFixed(2)}s"></span>`).join("");
  $("takeover").innerHTML = `${confetti}<div class="takeover-text"><div class="takeover-kicker">${escape(kicker)}</div><div class="takeover-value">${escape(value)}</div><div class="takeover-note">${escape(note)}</div></div>`;
  $("takeover").classList.add("show");
  setTimeout(() => {
    $("takeover").classList.remove("show");
    $("takeover").innerHTML = "";
    playAlerts();
  }, 14_000);
}

function prAlert(pr) {
  const implemented = pr.report?.implemented ?? 0;
  return { kicker: "PR merged", title: pr.title, meta: `#${pr.number} · ${pr.author}`, image: avatar(pr.avatar, 152), pill: implemented > 0 ? `+${fmt(implemented)} function${implemented > 1 ? "s" : ""}` : null };
}

function gameAlert(game) {
  return { kind: "game", kicker: "New game report", title: game.name, meta: game.stage ? `Furthest point: ${game.stage}` : "First test reported", icon: "🎮" };
}

// The first read of each list only records what is there: an alert means it happened while on air.
function detectChanges(state) {
  const fresh = (key, items, id) => {
    const known = seen[key];
    seen[key] = new Set([...(known ?? []), ...items.map(id)]);
    return known ? items.filter((item) => !known.has(id(item))) : [];
  };
  // First, so that alerts arriving with a milestone wait for it instead of playing behind it.
  if (state.progress) {
    const before = seen.done;
    seen.done = state.progress.done;
    if (before !== null && Math.floor(state.progress.done / MILESTONE_STEP) > Math.floor(before / MILESTONE_STEP)) {
      celebrate("Milestone reached", fmt(Math.floor(state.progress.done / MILESTONE_STEP) * MILESTONE_STEP), "PS5 system functions implemented");
      const reached = Math.floor(state.progress.done / MILESTONE_STEP) * MILESTONE_STEP;
      pushAlert(likeCta(`Like if you want to see ${fmt(reached + MILESTONE_STEP)}`));
    }
  }
  if (state.github) fresh("prs", state.github.mergedPrs, (pr) => pr.number).reverse().forEach((pr) => pushAlert(prAlert(pr)));
  if (state.games) fresh("games", state.games.recent, (game) => game.number).filter((game) => Date.now() - Date.parse(game.createdAt) < 3_600_000).reverse().forEach((game) => pushAlert(gameAlert(game)));
  const notable = state.events.filter((event) => event.kind === "complete" || event.kind === "release");
  fresh("events", notable, (event) => `${event.kind}|${event.title}`).reverse().forEach((event) => pushAlert({ kicker: event.kind === "release" ? "New release" : "Library finished", title: eventTitle(event.title), meta: event.detail ?? "", icon: event.kind === "release" ? "🚀" : "✅" }));
}

// Calls to action: tied to good news when there is some, and a light reminder otherwise.
const CTA_EVERY_MS = 3 * 60_000;
let ctaTurn = 0;

function likeGoal(likes) {
  const step = likes < 100 ? 25 : likes < 500 ? 50 : likes < 2000 ? 100 : 500;
  return (Math.floor(likes / step) + 1) * step;
}

function likeCta(title) {
  const likes = latest?.youtube?.likes;
  const meta = typeof likes === "number" ? `${fmt(likes)} / ${fmt(likeGoal(likes))} likes` : "It helps more people find AnyPS5";
  return { kind: "cta", kicker: "Enjoying the live?", title, meta, icon: "👍" };
}

function subscribeCta() {
  const shorts = latest?.youtube?.shorts;
  return { kind: "cta", kicker: "Stay in the loop", title: shorts ? "Subscribe for a daily AnyPS5 Short" : "Subscribe to follow AnyPS5's progress", meta: shorts ? "The day's progress, in 30 seconds" : "", icon: "🔔" };
}

function chatCta() {
  return { kind: "cta", kicker: "Ask the stream", title: "Type !gta for GTA VI's status", meta: "!game and a name for any other game. The answer shows up here", icon: "💬" };
}

function gta6Cta() {
  const gta = latest?.gta6;
  if (!gta) return likeCta("Hit like to support the stream");
  const meta = gta.stage === "released" ? `Needs PS5 firmware ${gta.firmware.required}, the public jailbreak stops at ${gta.firmware.jailbreak}` : gta.detail;
  return { kind: "cta", kicker: "GTA VI on AnyPS5", title: gta.title, meta, icon: "🌴" };
}

function discordCta() {
  return { kind: "discord", kicker: "Talk to the people building it", title: "Join the AnyPS5 Discord", meta: "discord.gg/BHFztBPUe · scan the code", image: "discord-qr.svg" };
}

function remindCta() {
  if (document.hidden || window.Gameplay?.playing()) return;
  const cards = [() => likeCta("Hit like to support the stream"), gta6Cta, hangarCta, subscribeCta, gta6Cta, discordCta];
  if (latest?.chat?.enabled) cards.splice(1, 0, chatCta);
  pushAlert(cards[ctaTurn % cards.length]());
  ctaTurn += 1;
}

function renderLikes(youtube) {
  const likes = youtube?.likes;
  const node = $("likes");
  node.hidden = typeof likes !== "number";
  if (node.hidden) return;
  const goal = likeGoal(likes);
  const before = seen.likes;
  seen.likes = likes;
  if (typeof before === "number" && likeGoal(before) <= likes) {
    pushAlert({ kind: "cta", kicker: "Like goal reached", title: `${fmt(likeGoal(before))} likes, thank you!`, meta: `Next goal: ${fmt(goal)} likes`, icon: "🎉" });
  }
  const start = goal - (likes < 100 ? 25 : likes < 500 ? 50 : likes < 2000 ? 100 : 500);
  node.innerHTML = `👍 ${fmt(likes)} / ${fmt(goal)}<span class="likes-bar"><span style="width:${(((likes - start) / (goal - start)) * 100).toFixed(1)}%"></span></span>`;
}

function runDemo(kind) {
  if (kind === "cta") {
    pushAlert(gta6Cta());
    pushAlert(hangarCta());
    pushAlert(likeCta("Hit like to support the stream"));
    pushAlert(subscribeCta());
  }
  const github = latest?.github;
  if (kind === "alert" && github?.mergedPrs?.[0]) {
    pushAlert(prAlert({ ...github.mergedPrs[0], report: { implemented: 12, declared: 0 } }));
    if (latest.games?.recent?.[0]) pushAlert(gameAlert(latest.games.recent[0]));
  }
  if (kind === "chat") {
    const game = latest?.games?.highlights?.[0];
    if (game) pushAlert({ kind: "answer game", kicker: "!game · asked by Viewer", title: game.name, meta: `${game.titleId ?? ""} · ${fmt(game.replies)} replies`, image: game.cover, pill: game.stage, progress: 100 });
    pushAlert({ kind: "discord", kicker: "!discord · asked by Viewer", title: "Join the AnyPS5 Discord", meta: "discord.gg/BHFztBPUe · scan the code", image: "discord-qr.svg" });
    const library = latest?.libraries?.all?.[0];
    if (library) pushAlert({ kind: "answer", kicker: "!lib · asked by Viewer", title: library.label, meta: `${fmt(library.done)} / ${fmt(library.total)} functions implemented`, icon: "📚", pill: `${Math.floor((library.done / library.total) * 100)}%`, progress: (library.done / library.total) * 100 });
    pushAlert({ kind: "cta", kicker: "!help · asked by Viewer", title: "!game  !lib  !next  !top  !random  !discord", meta: "Try !game elden ring: the answer shows up here", icon: "💬" });
  }
  if (kind === "milestone") celebrate("Milestone reached", "9,000", "PS5 system functions implemented");
}

// Answers go on screen as soon as the server has them, so this reads far more often than the rest of the state.
async function refreshChat() {
  try {
    const response = await fetch("/api/chat", { cache: "no-store" });
    if (!response.ok) return;
    const { answers } = await response.json();
    const known = seen.chat;
    seen.chat = new Set([...(known ?? []), ...answers.map((answer) => answer.id)]);
    if (known) answers.filter((answer) => !known.has(answer.id)).forEach((answer) => pushAlert({ ...answer, priority: true }));
  } catch (error) {
    console.error("chat", error);
  }
}

async function refresh() {
  try {
    const response = await fetch("/api/state", { cache: "no-store" });
    if (!response.ok) throw new Error(String(response.status));
    const state = await response.json();
    if (latest?.startedAt && state.startedAt !== latest.startedAt) reload();
    const first = !latest;
    latest = state;
    renderKpis(state);
    renderGtaChip(state.gta6);
    renderChart(state.history);
    renderPrs(state.github);
    renderEvents(state.events);
    renderGames(state.games);
    renderMap(state.libraries);
    renderLikes(state.youtube);
    detectChanges(state);
    if (first && params.get("demo")) runDemo(params.get("demo"));
  } catch (error) {
    console.error("refresh", error);
  }
}

tickClock();
setInterval(tickClock, 5000);
tickScenes();
setInterval(tickScenes, 1000);
setInterval(tickGtaSince, 1000);
refresh();
setInterval(refresh, REFRESH_MS);
refreshPerf();
setInterval(refreshPerf, PERF_REFRESH_MS);
refreshChat();
setInterval(refreshChat, 2000);
setInterval(remindCta, CTA_EVERY_MS);
// A clip playing at reload time ends first.
function reload() {
  if (window.Gameplay?.playing()) setTimeout(reload, 10_000);
  else location.reload();
}
setTimeout(reload, RELOAD_MS);
