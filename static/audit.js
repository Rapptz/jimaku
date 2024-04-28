const loadingElement = document.getElementById('loading');
const loadMore = document.getElementById('load-more');
const dtFormat = new Intl.DateTimeFormat(undefined, {
  dateStyle: 'full',
  timeStyle: 'medium',
});

settings.addEventListener('preferred-name', (value) => {
  let keys = {'romaji':'data-name', 'native':'data-japanese-name', 'english':'data-english-name'};
  let key = keys[value];
  document.querySelectorAll('.entry-name').forEach(el => {
    el.textContent = el.getAttribute(key) ?? el.getAttribute('data-name');
  });
});

const replaceOrAppend = (el, node) => Array.isArray(node) ? el.replaceChildren(...node) : el.appendChild(node);
const userLink = (account_id, info) => {
  let name = info.users[account_id];
  if(name) {
    return html('a', name, {href: `/user/${name}`});
  }
  return html('span.fallback', `User ID ${account_id}`);
}

const entryLink = (entry_id, info, fallback) => {
  let entry = info.entries[entry_id];
  if(entry) {
    let name = getPreferredNameForEntry(entry);
    return html('a.entry-name', name, {
      href: `/entry/${entry_id}`,
      dataset: {
        name: entry.name,
        japaneseName:
        entry.japanese_name,
        englishName: entry.english_name
      }
    });
  }
  return html('span.fallback', fallback != null ? fallback : entry_id);
}

const simplePlural = (c, s) => c === 1 ? `${c} ${s}` : `${c} ${s}s`;
const fileToElement = (op) => html('li.file', op.name, {class: op.failed ? 'failed' : 'success'});

const FLAG_NAMES = Object.freeze({
  anime: 'Anime',
  low_quality: 'Low Quality',
  external: 'Legacy',
  movie: 'Movie',
  adult: 'Adult',
});

let tmdbIdToUrl = (tmdbId) => {
  if(tmdbId == null) return null;
  let split = tmdbId.split(':');
  return `https://themoviedb.org/${split[0]}/${split[1]}`;
}

function auditLogEntry(id, title, contents) {
  const isEmpty = (e) => e == null || (Array.isArray(e) && e.length === 0)
  return html('details.audit-log-entry',
    isEmpty(contents) ? {class: 'empty'} : {},
    html('summary',
      html('.description',
        html('span.title', title),
        html('span.date', formatRelative(Math.floor(id / 1000)), {title: dtFormat.format(new Date(id))})
      ),
    ),
    html('.content', contents)
  );
}

const auditLogTypes = Object.freeze({
  scrape_result: (data, log, info) => {
    let title = data.error ? "Error scraping from Kitsunekko" : (data.directories.length === 0 ? "Checked Kitsunekko" : "Scraped from Kitsunekko");
    let elements = data.directories.map(d => {
      let link = d.anilist_id !== null ? html('a', d.name, {href: `https://anilist.co/anime/${d.anilist_id}/`}) : d.name;
      return html('li', link, ' (Original: ', html('span.original', d.original_name), ')');
    });
    let contents = elements.length === 0 ? null : html('ul', elements);
    return auditLogEntry(log.id, title, contents);
  },
  create_entry: (data, log, info) => {
    let title = [
      data.api ? "[API] " : "",
      userLink(log.account_id, info),
      " created ",
      data.anime ? "anime " : "live action ",
      "entry ",
      entryLink(log.entry_id, info, data.name),
    ];
    let content = [];
    if(data.tmdb_id) {
      let href = tmdbIdToUrl(data.tmdb_id);
      content.push(html('li', 'Using ', html('a', href, {href})));
    }
    if(data.anilist_id) {
      let href = `https://anilist.co/anime/${data.anilist_id}/`;
      content.push(html('li', 'Using ', html('a', href, {href})));
    }
    let contents = content.length === 0 ? null : html('ul', content);
    return auditLogEntry(log.id, title, contents);
  },
  move_entry: (data, log, info) => {
    let title = [
      userLink(log.account_id, info),
      " moved ",
      simplePlural(data.files.length, 'file'),
      " from ",
      entryLink(log.entry_id, info),
      " to ",
      entryLink(data.entry_id, info),
    ];

    let contents = [];
    if(data.created) {
      contents.push(html('li', 'Created a new ', data.anime ? 'anime ' : 'live action ', 'entry'));
    }
    if(data.tmdb_id) {
      let href = tmdbIdToUrl(data.tmdb_id);
      contents.push(html('li', 'Using ', html('a', href, {href})));
    }
    if(data.anilist_id) {
      let href = `https://anilist.co/anime/${data.anilist_id}/`;
      contents.push(html('li', 'Using ', html('a', href, {href})));
    }
    let files = data.files.map(fileToElement);
    contents.push(html('li', html('span', simplePlural(data.files.length, 'File'), ':'), html('ul', files)));
    return auditLogEntry(log.id, title, html('ul', contents));
  },
  rename_files: (data, log, info) => {
    let title = [
      userLink(log.account_id, info),
      " renamed ",
      simplePlural(data.files.length, 'file'),
      " in ",
      entryLink(log.entry_id, info),
    ];
    let contents = html('table',
      html('thead',
        html('tr',
          html('th', 'Status'),
          html('th', 'Original'),
          html('th', 'Renamed')
        )
      ),
      html('tbody',data.files.map((f) => {
        return html('tr',
          html('td', f.failed ? '\u2705\ufe0f' : '\u274c\ufe0f', {dataset: {th: 'Status'}}),
          html('td', f.from, {dataset: {th: 'Original'}}),
          html('td', f.to, {dataset: {th: 'Renamed'}})
        );
      }))
    );
    return auditLogEntry(log.id, title, contents);
  },
  upload: (data, log, info) => {
    let title = [
      data.api ? "[API] " : "",
      userLink(log.account_id, info),
      " uploaded ",
      simplePlural(data.files.length, 'file'),
      " in ",
      entryLink(log.entry_id, info),
    ];
    let files = data.files.map(fileToElement);
    return auditLogEntry(log.id, title, html('ul', files));
  },
  delete_files: (data, log, info) => {
    let title = [
      userLink(log.account_id, info),
      data.permanent ? " deleted " : " trashed ",
      simplePlural(data.files.length, 'file'),
      " in ",
      entryLink(log.entry_id, info),
    ];
    let contents = [];
    if(data.reason != null) {
      contents.push(html('span.reason', html('strong', 'Reason: '), data.reason));
    }
    contents.push(html('ul', data.files.map(fileToElement)));
    return auditLogEntry(log.id, title, contents);
  },
  delete_entry: (data, log, info) => {
    let title = [
      userLink(log.account_id, info),
      " deleted entry ",
      data.name
    ];
    return auditLogEntry(log.id, title, null);
  },
  trash_action: (data, log, info) => {
    let title = [
      userLink(log.account_id, info),
      data.restore ? " restored " : " permanently deleted ",
      simplePlural(data.files.length, 'file')
    ];
    let files = data.files.map(fileToElement);
    return auditLogEntry(log.id, title, html('ul', files));
  },
  edit_entry: (data, log, info) => {
    let title = [
      userLink(log.account_id, info),
      ' edited entry ',
      entryLink(log.entry_id, info),
    ];

    let elements = data.changed.map(key => {
      let before = data.before[key];
      let after = data.after[key];
      switch(key) {
      case 'name':
      case 'japanese_name':
      case 'english_name':
        let title = {name: 'name', japanese_name: 'Japanese name', english_name: 'English name'}[key];
        if(before == null && after != null) {
          return html('li', `Set a new ${title} `, html('span.after', after));
        } else if (before != null && after == null) {
          return html('li', `Removed the ${title} `, html('span.before', before));
        } else {
          return html('li', `Changed ${title} from `, html('span.before', before), ' to ', html('span.after', after));
        }
      case 'notes':
        if(before == null && after != null) {
          return html('li', 'Set the notes to ', html('code.after', html('pre', after)));
        } else if(before != null && after == null) {
          return html('li', 'Removed the notes')
        } else {
          return html('li', 'Changed the notes to ', html('code.after', html('pre', after)));
        }
      case 'anilist_id':
        if(before == null && after != null) {
          return html('li', 'Set AniList ID to ', html('a.after', after, {href: `https://anilist.co/anime/${after}/`}));
        } else if (before != null && after == null) {
          return html('li', 'Removed AniList ID ', html('a.before', before, {href: `https://anilist.co/anime/${before}/`}));
        } else {
          return html('li', 'Changed AniList ID from ',
                  html('a.before', before, {href: `https://anilist.co/anime/${before}/`}),
                  ' to ',
                  html('a.after', after, {href: `https://anilist.co/anime/${after}/`})
          );
        }
      case 'tmdb_id':
        before = tmdbIdToUrl(before);
        before = tmdbIdToUrl(after);
        if(before == null && after != null) {
          return html('li', 'Set TMDB URL to ', html('a.after', after, {href: after}));
        } else if (before != null && after == null) {
          return html('li', 'Removed TMDB URL ', html('a.before', before, {href: before}));
        } else {
          return html('li', 'Changed TMDB URL from ', html('a.before', before, {href: before}), ' to ', html('a.after', after, {href: after}));
        }
      case 'flags':
        let changes = [];
        for(const [flag, title] of Object.entries(FLAG_NAMES)) {
          let b = before[flag];
          let a = after[flag];
          if(b && !a) {
            changes.push(html('li', 'Removed ', html('span.before', title), ' flag'));
          } else if(!b && a) {
            changes.push(html('li', 'Added ', html('span.after', title), ' flag'));
          }
        }
        return changes;
      }
    });
    return auditLogEntry(log.id, title, html('ul', elements));
  }
});

function processData(info) {
  for(const log of info.logs) {
    let data = log.data;
    let parser = auditLogTypes[data.type];
    if(parser) {
      let node = parser(data, log, info);
      loadingElement.parentElement.insertBefore(node, loadingElement);
    }
  }

  loadMore.classList.remove("hidden");
  loadingElement.classList.add("hidden");
}

async function getAuditLogs(before) {
  loadingElement.textContent = 'Loading...';
  loadMore.textContent = "Loading...";
  loadMore.disabled = true;

  let params = new URL(document.location).searchParams;
  if(before) params.append('before', before);
  let response = await fetch('/audit-logs?' + params);
  if(response.status !== 200) {
    loadingElement.textContent = `Server responded with ${response.status}`;
    return;
  }

  let data = await response.json();
  processData(data);
  if(data.logs.length === 0) {
    loadMore.disabled = true;
    loadMore.textContent = "No more entries";
  } else {
    loadMore.textContent = "Load more";
    loadMore.dataset.lastId = data.logs[data.logs.length - 1].id;
    loadMore.disabled = false;
  }
}

document.addEventListener('DOMContentLoaded', () => getAuditLogs());
loadMore.addEventListener('click', () => getAuditLogs(loadMore.dataset.lastId))
