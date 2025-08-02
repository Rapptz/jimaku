/* This file is licensed under AGPL-3.0 */

const loadingElement = document.getElementById('loading');
const loadMore = document.getElementById('load-more');
const notificationsEl = document.getElementById('notifications');
const dtFormat = new Intl.DateTimeFormat(undefined, {
  dateStyle: 'full',
  timeStyle: 'medium',
});

settings.addEventListener('preferred-name', (value) => {
  let keys = {'romaji':'data-name', 'native':'data-japanese-name', 'english':'data-english-name'};
  let key = keys[value];
  document.querySelectorAll('.entry-name').forEach(el => {
    el.textContent = el.getAttribute(key) ?? el.dataset.name
  });
});

const simplePlural = (c, s) => c === 1 ? `${c} ${s}` : `${c} ${s}s`;
const downloadLink = (entryId, filename) => {
  return html('li.file',
    html('a', filename, {
      href: `/entry/${entryId}/download/${encodeURIComponent(filename)}`,
      title: filename,
    })
  );
}

const entryLink = (entry, entryId, fallback) => {
  if(entry == null) {
    return html('strong', fallback ?? 'Unknown Entry');
  }

  let id = entryId ?? entry.id;
  let name = getPreferredNameForEntry(entry);
  return html('a.entry-name', name,
    id != null ? { href: `/entry/${id}` } : null, {
    dataset: {
      name: entry.name,
      japaneseName: entry.japanese_name,
      englishName: entry.english_name,
    }
  });
}

const createNotification = (ack, timestamp, title, contents) =>{
  const isEmpty = (e) => e == null || (Array.isArray(e) && e.length === 0);
  return html('details.notification',
    isEmpty(contents) ? {class: 'empty'} : null,
    ack >= timestamp ? {class: 'acknowledged'} : null,
    html('summary',
      html('.description',
        html('span.title', title),
        html('span.date', formatRelative(Math.floor(timestamp / 1000)), {title: dtFormat.format(new Date(timestamp))}),
      )
    ),
    html('.content', contents)
  );
}

const notificationTypes = Object.freeze({
  new_subtitle: (payload, notification, data) => {
    let title = [
      simplePlural(payload.files.length, 'file'),
      ' uploaded in ',
      entryLink(notification.entry),
    ]

    let files = payload.files.map(name => downloadLink(notification.entry.id, name));
    let content = [
      html('ul', files),
      html('p.disclaimer', 'Some files may have been deleted or renamed due to moderation.')
    ]
    return createNotification(data.last_ack, notification.timestamp, title, content);
  },
  new_report: (payload, notification, data) => {
    let reportInfo = data.reports.find(r => r.report.id === payload.report_id);
    if(!reportInfo) {
      return null;
    }
    let report = reportInfo.report;

    let status = getReportStatusInfo(report.status);
    let title = [
      html('span.report-status.badge', {class: status.className }, status.text),
      html('a', {href: `/user/${reportInfo.account_name}`}, reportInfo.account_name),
      ' reported ',
      entryLink(reportInfo.entry, report.entry_id, report.payload.name)
    ];

    let content = [
      html('span.report-reason', report.reason),
    ];
    if(report.entry_id != null && report.payload.files.length !== 0) {
      let files = report.payload.files.map(name => downloadLink(report.entry_id, name));
      content.push(html('ul', files));
    }

    if(report.status === 0) {
      const button = html('button.button.primary', 'Respond');
      button.addEventListener('click', (e) => {
        e.preventDefault();
        const modal = document.getElementById('resolve-report-modal');
        if(modal != null) {
          modal.dataset.id = report.id;
          modal.showModal();
        }
      })
      content.push(button);
    }

    return createNotification(data.last_ack, notification.timestamp, title, content);
  },
  report_answered: (payload, notification, data) => {
    let reportInfo = data.reports.find(r => r.report.id === payload.report_id);
    if(!reportInfo) {
      return null;
    }
    let report = reportInfo.report;

    let status = getReportStatusInfo(report.status);
    let title = [
      html('span.report-status.badge', {class: status.className }, status.text),
      'Your report for ',
      entryLink(reportInfo.entry, report.entry_id, report.payload.name),
      ' has been answered.'
    ];

    let content = [
      html('span.report-reason', report.reason),
      html('span.report-response', report.response),
    ];

    if(report.entry_id != null && report.payload.files.length !== 0) {
      let files = report.payload.files.map(name => downloadLink(report.entry_id, name));
      content.push(html('ul', files));
    }

    return createNotification(data.last_ack, notification.timestamp, title, content);
  }
});

async function processData(data) {
  for(const notification of data.notifications) {
    let parser = notificationTypes[notification.payload.type];
    if(parser) {
      let node = parser(notification.payload, notification, data)
      if(node) {
        notificationsEl.appendChild(node);
      }
    }
  }

  loadMore.classList.remove("hidden");
  notificationsEl.classList.remove('hidden');
  loadingElement.classList.add("hidden");
}

async function getNotifications(before) {
  loadMore.textContent = 'Loading...';
  loadMore.disabled = true;

  let url = '/notifications/query';
  if(before)
    url += '?before=' + encodeURIComponent(before);

  const resp = await fetch(url);
  if(!resp.ok) {
    showAlert({level: 'error', content: `Server responded with ${resp.status}`});
    loadingElement.classList.add('hidden');
    return;
  }

  let result = await resp.json();
  await processData(result);
  if(result.notifications.length != 100) {
    if(before) {
      loadMore.disabled = true;
      loadMore.textContent = "No more entries";
    } else {
      loadMore.classList.add('hidden');
      if(result.notifications.length === 0) {
        auditLogEntries.appendChild(html('p', 'No entries!'));
      }
    }
  } else {
    loadMore.textContent = "Load more";
    loadMore.dataset.lastNotification = result.notifications[result.notifications.length - 1].ts;
    loadMore.disabled = false;
  }
}

async function ackNotifications() {
  const resp = await fetch('/notifications/ack', {method: 'POST'});
  if(resp.ok) {
    updateNotificationBadge(0);
  }
}

document.addEventListener('DOMContentLoaded', () => {
  ackNotifications();
  getNotifications();
});
loadMore.addEventListener('click', () => getNotifications(loadMore.dataset.lastNotification))
