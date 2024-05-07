/* This file is licensed under AGPL-3.0 */

const logSelect = document.getElementById('log-select');
// Statistics
const requestCount = document.getElementById('request-count');
const activeUsers = document.getElementById('active-users');
const averageResponseTime = document.getElementById('average-response-time');
const percentSuccess = document.getElementById('percent-success');
const registeredUsers = document.getElementById('registered-users');
const downloadCount = document.getElementById('download-count');
// Tables
const referringSites = document.getElementById('referring-sites');
const popularRoutes = document.getElementById('popular-routes');
const recentLogs = document.getElementById('recent-logs');
const popularApiRoutes = document.getElementById('popular-api-routes');
const popularApiUsers = document.getElementById('popular-api-users');

let baseUrl = `${window.location.protocol}//${window.location.host}`;
const styles = getComputedStyle(document.documentElement);
let logs = [];

const isMultiDay = () => logSelect.value.endsWith('-days');
const httpRequestLogs = () => logs.filter(data => data?.span?.name === 'http request');
const nonStaticHttpRequests = () => httpRequestLogs().filter(data => data?.span?.['http.url']?.startsWith('/static/') === false);
const logToUrl = (data) => {
  let url = data?.span?.['http.url'];
  return url != null ? new URL(url, baseUrl) : null;
}

async function getLogs(value) {
  let endpoint = '/admin/logs/today';
  if (value.endsWith('-days')) {
    endpoint = `/admin/logs?days=${value.substring(0, value.lastIndexOf('-'))}`;
  } else if (value !== 'today') {
    endpoint = `/admin/logs/${value}`;
  }

  logs = await callApi(endpoint);
}

function clearTable(table) {
  table.querySelector('tbody').innerHTML = '';
}

function getActiveUsers(requests) {
  let unique = new Set(requests.map(data => data?.span?.user_id).filter(e => typeof e == 'number'));
  return unique.size;
}

const stripPrefix = (s, prefix) => s.startsWith(prefix) ? s.slice(prefix.length) : s;
const isDownloadLog = (d) => /\/entry\/\d+\/(?:bulk|download)/.test(d?.span?.['http.url'] || "");

function getAverageResponseTime(requests) {
  let responseTimes = requests.map(data => (data?.span?.['http.latency'] ?? 0) / 1000);
  return Math.round(responseTimes.reduce((a, b) => a + b, 0) / responseTimes.length);
}

const isSpanSuccess = (data) => {
  let code = data?.span?.['http.status_code'];
  return code !== null && code >= 200 && code < 400;
}

const spanToUserData = (data) => {
  return { user_id: data?.span?.user_id, success: isSpanSuccess(data), url: logToUrl(data), download: isDownloadLog(data) };
}

function getSuccessRate(requests) {
  let successes = requests.reduce((a, data) => a + isSpanSuccess(data), 0);
  return successes / requests.length;
}

function getAppropriateTimeScales() {
  let unit = isMultiDay() ? 'day' : 'hour';
  return {
    x: {
      type: 'time',
      time: {
        unit,
        tooltipFormat: 'DD T',
      },
      title: {
        display: true,
        text: unit == 'day' ? 'Date' : 'Time',
      }
    },
    y: {
      title: {
        display: true,
        text: 'Total', // to override
      },
      ticks: {
        precision: 0,
      }
    }
  };
}

function getSearchEngine(url) {
  try {
    if (url.host.startsWith('google')) {
      return 'Google';
    } else if (url.host == 'bing.com') {
      return 'Bing';
    } else if (url.host == 'duckduckgo.com') {
      return 'DuckDuckGo';
    }
    return null;
  }
  catch (e) {
    return null;
  }
}

function getReferringSites(requests) {
  let counter = requests.map(d => d?.span?.['http.referrer'] || "").filter(r => r.indexOf(window.location.hostname) === -1 && r.length != 0).reduce((count, referrer) => {
    if (count.hasOwnProperty(referrer)) {
      count[referrer] += 1;
    } else {
      count[referrer] = 1;
    }
    return count;
  }, {});

  let tbody = referringSites.querySelector('tbody');
  tbody.innerHTML = '';
  for (const [referrer, count] of Object.entries(counter).sort(([, a], [, b]) => b - a).slice(0, 25)) {
    let tr = document.createElement('tr');
    let f = document.createElement('td');
    f.setAttribute('data-th', 'Site')
    if (referrer.startsWith('http')) {
      let url = new URL(referrer);
      let searchEngine = getSearchEngine(url);
      if (searchEngine === null) {
        let a = document.createElement('a');
        a.href = referrer;
        a.textContent = url.host;
        f.appendChild(a);
      } else {
        f.textContent = searchEngine;
      }
    } else {
      f.textContent = referrer;
    }
    let c = document.createElement('td');
    c.setAttribute('data-th', 'Views');
    c.textContent = count.toLocaleString();
    tr.appendChild(f);
    tr.appendChild(c);
    tbody.appendChild(tr);
  }
}

function getPopularRoutes(requests) {
  let counter = requests.map(logToUrl).filter(url => url !== null).reduce((count, url) => {
    route = url.pathname;
    if (count.hasOwnProperty(route)) {
      count[route] += 1;
    } else {
      count[route] = 1;
    }
    return count;
  }, {});

  let tbody = popularRoutes.querySelector('tbody');
  tbody.innerHTML = '';
  for (const [route, count] of Object.entries(counter).sort(([, a], [, b]) => b - a).slice(0, 25)) {
    let tr = document.createElement('tr');
    let f = document.createElement('td');
    f.setAttribute('data-th', 'Route')
    let a = document.createElement('a');
    a.href = route;
    a.textContent = route;
    f.appendChild(a);
    let c = document.createElement('td');
    c.setAttribute('data-th', 'Views');
    c.textContent = count.toLocaleString();
    tr.appendChild(f);
    tr.appendChild(c);
    tbody.appendChild(tr);
  }
}

function getPopularApiRoutes(requests) {
  let counter = requests.map(logToUrl).filter(url => url !== null && url.pathname.startsWith('/api/')).reduce((count, url) => {
    route = url.pathname;
    if (count.hasOwnProperty(route)) {
      count[route] += 1;
    } else {
      count[route] = 1;
    }
    return count;
  }, {});

  let tbody = popularApiRoutes.querySelector('tbody');
  tbody.innerHTML = '';
  for (const [route, count] of Object.entries(counter).sort(([, a], [, b]) => b - a).slice(0, 25)) {
    tbody.appendChild(html('tr',
      html('td', route, { dataset: { th: 'Route' } }),
      html('td', count.toLocaleString(), { dataset: { th: 'Calls' } })
    ));
  }
}

function getTopApiUsers(requests) {
  let counter = requests.map(spanToUserData)
    .filter(data => data.user_id != null && data.url !== null && (data.download || data.url.pathname.startsWith('/api/')))
    .reduce((count, data) => {
      let key = data.user_id;
      if (count.hasOwnProperty(key)) {
        let subkey = data.success ? 'success' : 'failed';
        count[key][subkey] += 1;
      } else {
        count[key] = { success: data.success, failed: !data.success };
      }
      return count;
    }, {});

  let tbody = popularApiUsers.querySelector('tbody');
  tbody.innerHTML = '';
  for (const [user_id, counts] of Object.entries(counter).sort(([, a], [, b]) => (b.success + b.failed) - (a.success + a.failed)).slice(0, 25)) {
    tbody.appendChild(html('tr',
      html('td', html('a', user_id, { href: `/admin/user/${user_id}` }), { dataset: { th: 'User ID' } }),
      html('td', counts.success + counts.failed, { dataset: { th: 'Total' } }),
      html('td', counts.success, { dataset: { th: 'Success' } }),
      html('td', counts.failed || '0', { dataset: { th: 'Failed' } }),
    ));
  }
}

function getRecentServerLogs() {
  const formatValue = (x) => typeof x === 'string' ? JSON.stringify(x) : x.toString();

  let filtered = logs.filter(log => log.target !== "jimaku::logging").reverse().slice(0, 25);
  let tbody = recentLogs.querySelector('tbody');
  tbody.innerHTML = '';
  for (const log of filtered) {
    let tr = document.createElement('tr');
    let ts = document.createElement('td');
    ts.setAttribute('data-th', 'Timestamp');
    ts.setAttribute('title', log.timestamp);
    ts.textContent = formatRelative(Math.floor(Date.parse(log.timestamp) / 1000));
    let level = document.createElement('td');
    level.setAttribute('data-th', 'Level');
    level.textContent = log.level;
    level.classList.add(log.level.toLowerCase());
    let target = document.createElement('td');
    target.setAttribute('data-th', 'Target');
    target.textContent = log.target;
    let message = document.createElement('td');
    message.setAttribute('data-th', 'Message');
    message.textContent = log.fields?.message ?? "Nothing";
    let fields = document.createElement('td');
    fields.setAttribute('data-th', 'Fields');
    fields.textContent = Object.entries(log.fields).filter(([name, _]) => name != 'message').map(([name, value]) => `${name}=${formatValue(value)}`).join(", ");
    tr.appendChild(ts);
    tr.appendChild(level);
    tr.appendChild(target);
    tr.appendChild(message);
    tr.appendChild(fields);
    tbody.appendChild(tr);
  }
}

function updateGraphs() {
  let requests = nonStaticHttpRequests();
  requestCount.textContent = requests.length.toLocaleString();
  activeUsers.textContent = getActiveUsers(requests);
  averageResponseTime.textContent = `${getAverageResponseTime(requests)} ms`;
  percentSuccess.textContent = getSuccessRate(requests).toLocaleString(undefined, { style: 'percent', minimumFractionDigits: 2 });
  downloadCount.textContent = requests.filter(isDownloadLog).length.toLocaleString();
  getReferringSites(requests);
  getPopularRoutes(requests);
  getPopularApiRoutes(requests);
  getTopApiUsers(requests);
  getRecentServerLogs();
}

logSelect.addEventListener('change', async () => {
  await getLogs(logSelect.value);
  updateGraphs();
});

getLogs(logSelect.value).then(updateGraphs);
