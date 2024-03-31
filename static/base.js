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

function detectOS() {
  const userAgent = window.navigator.userAgent,
    windowsPlatforms = ['Win32', 'Win64', 'Windows'],
    iosPlatforms = ['iPhone', 'iPad', 'iPod'];

  if (/Macintosh/.test(userAgent)) {
    return 'macOS';
  } else if (iosPlatforms.indexOf(userAgent) !== -1) {
    return 'iOS';
  } else if (windowsPlatforms.indexOf(userAgent) !== -1) {
    return 'windows';
  } else if (/Android/.test(userAgent)) {
    return 'android';
  } else if (/Linux/.test(platform)) {
    return 'linux';
  } else {
    return null;
  }
}

const defaultAlertHook = (alert) => main.insertBefore(alert, main.firstChild);

async function callApi(url, options, alertHook) {
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
      return await resp.text();
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
