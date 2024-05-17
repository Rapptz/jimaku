/* This file is licensed under AGPL-3.0 */
const anilistRegex = /^https:\/\/anilist\.co\/anime\/(\d+)\//m;
const tmdbRegex = /^https:\/\/(?:www\.)?themoviedb\.org\/(tv|movie)\/(\d+)(?:-[a-zA-Z0-9\-]+)?(?:\/.*)?/m;
const main = document.querySelector('main');
const settings = document.getElementById('settings');
const settingsModal = document.getElementById('settings-modal');
const rtf = new Intl.RelativeTimeFormat(undefined, {
  style: 'long', numberic: 'auto',
});

function __parseQuery(query) {
  let chunks = query.split(/([.#])/);
  let classList = [];
  let id = null;
  for(let i = 1; i < chunks.length; i += 2) {
    if(chunks[i] === '.') {
      classList.push(chunks[i + 1]);
    } else if(chunks[i] === '#') {
      id = chunks[i + 1];
    }
  }
  return {classList, tag: chunks[0] || 'div', id};
}

function __create(query) {
  const {classList, tag, id} = __parseQuery(query);
  let el = document.createElement(tag);
  if(id !== null) el.id = id;
  if(classList.length !== 0) el.classList.add(...classList);
  return el;
}

function __setData(el, key, value) {
  if(typeof key === 'object') {
    for(const k in key) {
      __setData(el, k, key[k]);
    }
    return;
  }
  if(value == null) {
    delete el.dataset[key];
  } else{
    el.dataset[key] = value;
  }
}

function __setAttr(el, key, value) {
  if(typeof key === 'object') {
    for(const k in key) {
      __setAttr(el, k, key[k]);
    }
    return;
  }
  const isFunc = typeof value === 'function';
  if(key === 'dataset') {
    __setData(el, value);
  } else if((key in el || isFunc) && (key !== 'list')) {
    el[key] = value;
  } else {
    if(el.className && key === 'class') {
      value = `${el.className} ${value}`;
    }
    if(value == null) {
      el.removeAttribute(key);
    } else {
      el.setAttribute(key, value);
    }
  }
}

function __expandArgs(el, args) {
  for(const arg of args) {
    const type = typeof arg;
    if(type === 'function') {
      arg(el);
    } else if(type === 'string' || type === 'number') {
      el.appendChild(new Text(arg ?? ''));
    } else if(arg && arg.nodeType) {
      el.appendChild(arg);
    } else if(Array.isArray(arg)) {
      __expandArgs(el, arg);
    } else if(type === 'object') {
      __setAttr(el, arg, null);
    }
  }
}

function html(query, ...args) {
  let el = __create(query);
  __expandArgs(el, args);
  return el;
}

const getAnilistId = (url) => {
  const m = url.match(anilistRegex);
  return m == null || m.length !== 2 ? null : parseInt(m[1], 10);
}

const getTmdbId = (url) => {
  const m = url.match(tmdbRegex);
  return m == null || m.length !== 3 ? null : { type: m[1], id: parseInt(m[2], 10) };
}

function debounced(func, timeout = 300) {
  let timer;
  return (...args) => {
    clearTimeout(timer);
    timer = setTimeout(() => {
      func.apply(this, args)
    }, timeout)
  }
};

function formatRelative(seconds) {
  const dt = Math.round(seconds - (Date.now() / 1000));
  const cutoffs = [60, 3600, 86400, 86400 * 7, 86400 * 28, 86400 * 365, Infinity];
  const units = ['second', 'minute', 'hour', 'day', 'week', 'month', 'year'];
  const index = cutoffs.findIndex(v => v > Math.abs(dt));
  const divisor = index ? cutoffs[index - 1] : 1;
  return rtf.format(Math.floor(dt / divisor), units[index]);
}

function createAlert({content, level = 'info'}) {
  return html('div.alert', {role: 'alert', class: level},
    content ? html('p', content) : null,
    html('button.close', {
      'aria-hidden': 'true',
      type: 'button',
      onclick: closeAlert,
  }));
}

function closeAlert(e) {
  e.preventDefault();
  let d = e.target.parentElement;
  let p = d?.parentElement;
  p?.removeChild(d);
}

function showAlert({ level, content }) {
  const alert = createAlert({ level, content });
  main.insertBefore(alert, main.firstChild);
}

function detectOS(ua) {
  const userAgent = ua || window.navigator.userAgent,
    windowsPlatforms = ['Win32', 'Win64', 'Windows'],
    iosPlatforms = ['iPhone', 'iPad', 'iPod'];

  if (userAgent.indexOf('Macintosh') !== -1) {
    if(navigator.standalone && navigator.maxTouchPoints > 2) {
      return 'iPadOS';
    }
    return 'macOS';
  } else if (iosPlatforms.some(p => userAgent.indexOf(p) !== -1)) {
    return 'iOS';
  } else if (windowsPlatforms.some(p => userAgent.indexOf(p) !== -1)) {
    return 'Windows';
  } else if (userAgent.indexOf('Android') !== -1) {
    return 'Android';
  } else if (userAgent.indexOf('Linux') !== -1) {
    return 'Linux';
  } else {
    return null;
  }
}

function detectBrowser(ua) {
  const userAgent = ua || window.navigator.userAgent;
  let match = userAgent.match(/(opera|chrome|safari|firefox|msie|trident(?=\/))\/?\s*(\d+)/i) || [];
  if(/trident/i.test(match[1])) {
    return 'Internet Explorer';
  }
  if(match[1] == 'Chrome') {
    let inner = userAgent.match(/\b(OPR|Edge)\/(\d+)/);
    if(inner !== null) {
      return inner[1].replace('OPR', 'Opera');
    }
    if(/\bEdg\/\d+/.test(userAgent)) {
      return 'Edge';
    }
    return 'Chrome';
  }
  return match[1];
}

function deviceDescription() {
  const ua = window.navigator.userAgent;
  let browser = detectBrowser(ua);
  let os = detectOS(ua);
  if(!browser && !os) {
    return '';
  }
  browser = browser ?? 'Unknown Browser';
  return !!os ? `${browser} on ${os}` : browser;
}

const defaultAlertHook = (alert) => main.insertBefore(alert, main.firstChild);

async function callApi(url, options, alertHook, blob = false) {
  let resp = await fetch(url, options);
  let hook = alertHook ?? defaultAlertHook;

  if(!resp.ok) {
    let content = `Server responded with status code ${resp.status}`;
    if(resp.headers.get('content-type') === 'application/json') {
      let js = await resp.json();
      content = js.error;
    }
    let alert = createAlert({level: 'error', content});
    hook(alert);
    return null;
  } else {
     if(resp.headers.get('content-type') === 'application/json') {
      return await resp.json();
    } else {
      return blob ? await resp.blob() : await resp.text();
    }
  }
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

settings.addEventListener('click', () => settingsModal.showModal());
const preferredName = document.getElementById('preferred-name');
preferredName.value = localStorage.getItem('preferred_name') ?? 'romaji';
preferredName.addEventListener('change', () => {
  localStorage.setItem('preferred_name', preferredName.value);
  settings.dispatchEvent(new CustomEvent('preferred-name', {detail: preferredName.value}));
});

const initialSortBy = document.getElementById('initial-sort-by');
initialSortBy.value = localStorage.getItem('initial-sort-by') ?? 'name';
initialSortBy.addEventListener('change', () => {
  localStorage.setItem('initial-sort-by', initialSortBy.value);
});

const initialSortOrder = document.getElementById('initial-sort-order');
initialSortOrder.value = localStorage.getItem('initial-sort-order') ?? 'ascending';
initialSortOrder.addEventListener('change', () => {
  localStorage.setItem('initial-sort-order', initialSortOrder.value);
});

function getPreferredNameForEntry(entry) {
  let value = localStorage.getItem('preferred_name') ?? 'romaji';
  if(value == 'romaji') return entry.name;
  if(value == 'native') return entry.japanese_name ?? entry.name;
  if(value == 'english') return entry.english_name ?? entry.name;
  return entry.name;
}
