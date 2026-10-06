const btn = document.querySelector("#theme-toggle");
const html = document.documentElement;

const saved = localStorage.getItem("theme");
const osDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
html.dataset.theme = saved || (osDark ? "dark" : "light");

btn.addEventListener("click", () => {
  const next = html.dataset.theme === "dark" ? "light" : "dark";
  html.dataset.theme = next;
  localStorage.setItem("theme", next);
});

const LOCALES = [
  {
    code: "en",
    name: "English",
    englishName: "English",
    currency: "$",
    currencyCode: "USD",
  },
  {
    code: "pl",
    name: "Polski",
    englishName: "Polish",
    currency: "zł",
    currencyCode: "PLN",
  },

  // TODO translations
  // { code: "zh", name: "中文", englishName: "Chinese", currency: "¥", currencyCode: "CNY" },
  // { code: "ja", name: "日本語", englishName: "Japanese", currency: "¥", currencyCode: "JPY" },
  // { code: "hi", name: "हिन्दी", englishName: "Hindi", currency: "₹", currencyCode: "INR" },
  //
  // { code: "de", name: "Deutsch", englishName: "German", currency: "€", currencyCode: "EUR" },
  // { code: "fr", name: "Français", englishName: "French", currency: "€", currencyCode: "EUR" },
  // { code: "es", name: "Español", englishName: "Spanish", currency: "€", currencyCode: "EUR" },
  // { code: "it", name: "Italiano", englishName: "Italian", currency: "€", currencyCode: "EUR" },
  // { code: "pt", name: "Português", englishName: "Portuguese", currency: "€", currencyCode: "EUR" },
  // { code: "nl", name: "Nederlands", englishName: "Dutch", currency: "€", currencyCode: "EUR" },
  //
  // { code: "ko", name: "한국어", englishName: "Korean", currency: "₩", currencyCode: "KRW" },
  // { code: "ru", name: "Русский", englishName: "Russian", currency: "₽", currencyCode: "RUB" },
  // { code: "uk", name: "Українська", englishName: "Ukrainian", currency: "₴", currencyCode: "UAH" },
  // { code: "tr", name: "Türkçe", englishName: "Turkish", currency: "₺", currencyCode: "TRY" },
  //
  // { code: "ar", name: "العربية", englishName: "Arabic", currency: "﷼", currencyCode: "SAR" },
  // { code: "he", name: "עברית", englishName: "Hebrew", currency: "₪", currencyCode: "ILS" },
  // { code: "th", name: "ไทย", englishName: "Thai", currency: "฿", currencyCode: "THB" },
  // { code: "vi", name: "Tiếng Việt", englishName: "Vietnamese", currency: "₫", currencyCode: "VND" },
  // { code: "id", name: "Bahasa Indonesia", englishName: "Indonesian", currency: "Rp", currencyCode: "IDR" },
  //
  // { code: "sv", name: "Svenska", englishName: "Swedish", currency: "kr", currencyCode: "SEK" },
  // { code: "da", name: "Dansk", englishName: "Danish", currency: "kr", currencyCode: "DKK" },
  // { code: "no", name: "Norsk", englishName: "Norwegian", currency: "kr", currencyCode: "NOK" },
  // { code: "fi", name: "Suomi", englishName: "Finnish", currency: "€", currencyCode: "EUR" },
  //
  // { code: "cs", name: "Čeština", englishName: "Czech", currency: "Kč", currencyCode: "CZK" },
  // { code: "sk", name: "Slovenčina", englishName: "Slovak", currency: "€", currencyCode: "EUR" },
  // { code: "hu", name: "Magyar", englishName: "Hungarian", currency: "Ft", currencyCode: "HUF" },
  // { code: "ro", name: "Română", englishName: "Romanian", currency: "lei", currencyCode: "RON" },
  // { code: "bg", name: "Български", englishName: "Bulgarian", currency: "лв", currencyCode: "BGN" },
  //
  // { code: "hr", name: "Hrvatski", englishName: "Croatian", currency: "€", currencyCode: "EUR" },
  // { code: "sl", name: "Slovenščina", englishName: "Slovenian", currency: "€", currencyCode: "EUR" },
  // { code: "el", name: "Ελληνικά", englishName: "Greek", currency: "€", currencyCode: "EUR" },
  //
  // { code: "ms", name: "Bahasa Melayu", englishName: "Malay", currency: "RM", currencyCode: "MYR" },
  // { code: "fil", name: "Filipino", englishName: "Filipino", currency: "₱", currencyCode: "PHP" },
  // { code: "bn", name: "বাংলা", englishName: "Bengali", currency: "৳", currencyCode: "BDT" },
  // { code: "ur", name: "اردو", englishName: "Urdu", currency: "₨", currencyCode: "PKR" },
  //
  // { code: "sw", name: "Kiswahili", englishName: "Swahili", currency: "TSh", currencyCode: "TZS" },
  //
  // { code: "ca", name: "Català", englishName: "Catalan", currency: "€", currencyCode: "EUR" },
  // { code: "eu", name: "Euskara", englishName: "Basque", currency: "€", currencyCode: "EUR" },
  // { code: "gl", name: "Galego", englishName: "Galician", currency: "€", currencyCode: "EUR" },
];
function displayName(l) {
  return l.name === l.englishName ? l.name : `${l.name} (${l.englishName})`;
}

const FALLBACK = "en";
const cache = {};

// function getNested(obj, path) {
//   return path.split(".").reduce((o, k) => (o && o[k] !== undefined ? o[k] : undefined), obj);
// }

function getNested(obj, path, seen = new Set()) {
  if (seen.has(path)) {
    console.warn(`Circular translation reference: ${path}`);
    return undefined;
  }

  seen.add(path);

  const value = path
    .split(".")
    .reduce((o, k) => (o && o[k] !== undefined ? o[k] : undefined), obj);

  if (typeof value === "string" && value.startsWith("@:")) {
    return getNested(obj, value.slice(2), seen);
  }

  return value;
}

async function loadLocale(code) {
  if (cache[code]) return cache[code];
  const res = await fetch(`/locales/${code}.json`);
  if (!res.ok) throw new Error(`Missing locale file: ${code}`);
  const data = await res.json();
  cache[code] = data;
  return data;
}

function applyTranslations(dict) {
  document.querySelectorAll("[data-i18n]").forEach((el) => {
    const key = el.getAttribute("data-i18n");
    const val = getNested(dict, key);
    if (val !== undefined) el.textContent = val;
  });

  document.querySelectorAll("[data-i18n-attr]").forEach((el) => {
    let map;
    try {
      map = JSON.parse(el.getAttribute("data-i18n-attr"));
    } catch {
      return;
    }
    Object.entries(map).forEach(([attr, key]) => {
      const val = getNested(dict, key);
      if (val !== undefined) el.setAttribute(attr, val);
    });
  });
}
async function setLocale(code) {
  let dict;
  try {
    dict = await loadLocale(code);
  } catch {
    if (code !== FALLBACK) return setLocale(FALLBACK);
    return;
  }
  applyTranslations(dict);
  updateDownloadButtons(dict);

  document.documentElement.setAttribute("lang", code);
  document.documentElement.setAttribute("data-i18n-ready", "true"); // NEW — unhide body
  localStorage.setItem("lang", code);
  document.dispatchEvent(new CustomEvent("localechange", { detail: code }));
  const label = document.getElementById("lang-current-label");
  const found = LOCALES.find((l) => l.code === code);
  if (label) label.textContent = found ? displayName(found) : code;

  updateCurrency();
}

function detectOS() {
  const ua = navigator.userAgent || "";
  const platform =
    navigator.platform || navigator.userAgentData?.platform || "";

  if (/android/i.test(ua)) return "android";
  if (/iphone|ipad|ipod/i.test(ua)) return "ios";
  if (/mac/i.test(platform) && !/iphone|ipad|ipod/i.test(ua)) return "mac";
  if (/win/i.test(platform)) return "windows";
  if (/linux/i.test(platform)) return "linux";
  return null;
}

const OS_NAMES = {
  windows: "Windows",
  mac: "macOS",
  linux: "Linux",
  android: "Android",
  ios: "iOS",
};

function updateDownloadButtons(dict) {
  const os = detectOS();
  const genericLabel =
    getNested(dict, "website.download_generic") || "Download";
  let label = genericLabel;

  if (os) {
    const osName = OS_NAMES[os];
    const template = getNested(dict, "website.download_fallback");
    if (osName && template) label = template.replace("{}", osName);
  }

  document.querySelectorAll("[data-i18n-download]").forEach((el) => {
    el.textContent = label;
    if (os) el.setAttribute("data-detected-os", os);
  });
}

function detectInitialLocale() {
  const saved = localStorage.getItem("lang");
  if (saved && LOCALES.some((l) => l.code === saved)) return saved;
  const nav = (navigator.language || FALLBACK).slice(0, 2);
  return LOCALES.some((l) => l.code === nav) ? nav : FALLBACK;
}

function buildSelector() {
  const wrap = document.createElement("div");
  wrap.className = "lang-select";
  wrap.innerHTML = `
    <button type="button" class="lang-select-btn" aria-haspopup="listbox" aria-expanded="false">
      <span id="lang-current-label">English</span>
    </button>
    <div class="lang-select-panel" hidden>
      <input type="text" class="lang-select-search" placeholder="Search language…" />
      <ul class="lang-select-list" role="listbox"></ul>
    </div>
  `;

  const btn = wrap.querySelector(".lang-select-btn");
  const panel = wrap.querySelector(".lang-select-panel");
  const search = wrap.querySelector(".lang-select-search");
  const list = wrap.querySelector(".lang-select-list");
  let currentMatches = [];
  let highlightIndex = 0;

  function highlight(index) {
    const items = list.querySelectorAll("li");
    items.forEach((li, i) => li.classList.toggle("active", i === index));
    if (items[index]) items[index].scrollIntoView({ block: "nearest" });
  }

  function renderList(filter = "") {
    const q = filter.trim().toLowerCase();
    list.innerHTML = "";
    currentMatches = LOCALES.filter(
      (l) =>
        l.name.toLowerCase().includes(q) ||
        l.englishName.toLowerCase().includes(q) ||
        l.code.includes(q),
    );
    currentMatches.forEach((l) => {
      const li = document.createElement("li");
      li.textContent = displayName(l);
      li.setAttribute("role", "option");
      li.tabIndex = 0;
      li.addEventListener("click", () => {
        setLocale(l.code);
        closePanel();
      });
      list.appendChild(li);
    });
    highlightIndex = 0;
    highlight(highlightIndex);
  }

  function openPanel() {
    panel.hidden = false;
    btn.setAttribute("aria-expanded", "true");
    renderList();
    search.value = "";
    search.focus();
  }
  function closePanel() {
    panel.hidden = true;
    btn.setAttribute("aria-expanded", "false");
  }

  btn.addEventListener("click", () =>
    panel.hidden ? openPanel() : closePanel(),
  );
  search.addEventListener("input", (e) => renderList(e.target.value));
  search.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      const pick = currentMatches[highlightIndex];
      if (pick) {
        setLocale(pick.code);
        closePanel();
      }
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      highlightIndex = Math.min(highlightIndex + 1, currentMatches.length - 1);
      highlight(highlightIndex);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      highlightIndex = Math.max(highlightIndex - 1, 0);
      highlight(highlightIndex);
    }
  });
  document.addEventListener("click", (e) => {
    if (!wrap.contains(e.target)) closePanel();
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") closePanel();
  });

  return wrap;
}

document.addEventListener("DOMContentLoaded", () => {
  const mount = document.querySelector(".tabs") || document.body;
  mount.appendChild(buildSelector());
  setLocale(detectInitialLocale());
});

function updateCurrency() {
  const currency = getCurrency();

  document.querySelectorAll("[data-currency]").forEach((el) => {
    el.textContent = currency.symbol;
  });

  document.querySelectorAll("[data-currency-code]").forEach((el) => {
    el.textContent = currency.code;
  });
}
function getCurrency() {
  const code = localStorage.getItem("lang") || FALLBACK;
  const found =
    LOCALES.find((l) => l.code === code) ||
    LOCALES.find((l) => l.code === FALLBACK);
  return { symbol: found.currency, code: found.currencyCode };
}
