// @license magnet:?xt=urn:btih:0b31508aeb0634b347b8270c7bee4d411b5d4109&dn=agpl-3.0.txt AGPL-v3-or-Later
const anilistRegex = /^https:\/\/anilist\.co\/anime\/(\d+)\//m;
const tmdbRegex = /^https:\/\/(?:www\.)?themoviedb\.org\/(tv|movie)\/(\d+)(?:-[a-zA-Z0-9\-]+)?(?:\/.*)?/m;
const main = document.querySelector('main');
const settings = document.getElementById('settings');
const settingsModal = document.getElementById('settings-modal');
const rtf = new Intl.RelativeTimeFormat(undefined, {
  style: 'long', numberic: 'auto',
});

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

/**
 * @param  {Object} options
 * @param  {string?} options.level
 * @param  {string?} options.content
 * @return {HTMLDivElement}
 */
function createAlert({content, level = 'info'}) {
  const div = document.createElement('div');
  div.classList.add('alert');
  div.classList.add(level);
  div.setAttribute('role', 'alert');
  if (content) {
    const p = document.createElement('p');
    p.innerHTML = content;
    div.appendChild(p);
  }
  const button = document.createElement('button');
  button.setAttribute('aria-hidden', 'true');
  button.setAttribute('type', 'button');
  button.classList.add('close');
  button.addEventListener('click', () => div.parentElement.removeChild(div));
  div.appendChild(button);
  return div
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

function getPreferredNameForEntry(entry) {
  let value = localStorage.getItem('preferred_name') ?? 'romaji';
  if(value == 'romaji') return entry.name;
  if(value == 'native') return entry.japanese_name ?? entry.name;
  if(value == 'english') return entry.english_name ?? entry.name;
  return entry.name;
}
// @license-end
