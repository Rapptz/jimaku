/* This file is licensed under AGPL-3.0 */
import { init, parse } from "./anitomy.js";

const entriesElement = document.getElementById('anilist-entries');
const loadingElement = document.getElementById('loading');
const loadMoreButton = document.getElementById('load-more');
let afterIndex = 0;
const anilistQuery = `
query ($username: String) {
  MediaListCollection(
    forceSingleCompletedList: true
    userName: $username
    status_in: [CURRENT, REPEATING, PLANNING]
    type: ANIME
  ) {
    lists {
      name
      isCustomList
      entries {
        mediaId
        status
        progress
        media {
          coverImage {
            extraLarge
            medium
          }
          episodes
          status(version: 2)
        }
      }
    }
  }
}
`;

settings.addEventListener('preferred-name', (value) => {
  let keys = {'romaji':'data-name', 'native':'data-japanese-name', 'english':'data-english-name'};
  let key = keys[value.detail];
  document.querySelectorAll('.entry-name').forEach(el => {
    el.textContent = el.getAttribute(key) ?? el.dataset.name
  });
});

async function getAniListEntries(username) {
  let variables = {
    username,
  };

  let result = [];
  let resp = await fetch('https://graphql.anilist.co', {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
    },
    body: JSON.stringify({
      query: anilistQuery,
      variables,
    })
  });
  let json = await resp.json();
  if(resp.status !== 200) {
    let error = json.errors[0].message;
    if(error === "Private User" && resp.status === 404) {
      throw new Error(`This user's AniList page is private, and cannot be accessed.`);
    }
    else {
      throw new Error(`This user's AniList page could not be found or some other error happened, sorry.`);
    }
  }

  let lists = json?.data?.MediaListCollection?.lists?.filter(d => !d.isCustomList) ?? [];
  result.push(...lists.map(d => d.entries.filter(m => m.media.status !== 'NOT_YET_RELEASED')).flat());
  return result;
}

function humanFileSize(size) {
    var i = size == 0 ? 0 : Math.floor(Math.log(size) / Math.log(1000));
    return Number((size / Math.pow(1000, i)).toFixed(2)) + ' ' + ['B', 'kB', 'MB', 'GB', 'TB'][i];
}

const entryLink = (entry) => {
  let name = getPreferredNameForEntry(entry);
  return html('a.entry-name', name, {
    href: `/entry/${entry.id}`,
    dataset: {
      name: entry.name,
      japaneseName:
      entry.japanese_name,
      englishName: entry.english_name
    }
  });
}

function isValidFile(filename, progress) {
  let parsed = parse(filename);
  if(parsed.episode) {
    if(Array.isArray(parsed.episode)) {
      let start = parseInt(parsed.episode[0]);
      let end = parseInt(parsed.episode[1]);
      let nextEpisode = progress + 1;
      return nextEpisode >= start && nextEpisode <= end;
    }
    let value = parseInt(parsed.episode);
    return value > progress;
  }
  return true;
}

function anilistEntryToElement(data, entry, files) {
  let isHiding = false;
  let table = html('div.files', {dataset: {columns: '4'}},
    html('div.table-headers',
      html('span.table-header', html('input.bulk-check', {type: 'checkbox', autocomplete: 'off'})),
      html('span.table-header.sorting-ascending', {dataset: {sortBy: 'name'}}, 'Name'),
      html('span.table-header', {dataset: {sortBy: 'size'}}, 'Size'),
      html('span.table-header', {dataset: {sortBy: 'last-modified'}}, 'Date'),
    ),
    files.map(file => {
      let date = Date.parse(file.last_modified);
      let hidden = data.progress !== 0 ? !isValidFile(file.name, data.progress) : false;
      isHiding = isHiding || hidden;
      return html('div.entry', {dataset: {name: file.name, size: file.size, lastModified: date}},
        hidden ? {class: 'hidden filtered-episode'} : null,
        html('span.table-data.file-bulk', html('input', {autocomplete: 'off', type: 'checkbox'})),
        html('a.table-data.file-name', file.name, {href: file.url }),
        html('span.table-data.file-size', humanFileSize(file.size)),
        html('span.table-data.file-modified', {title: file.last_modified}, formatRelative(date / 1000))
      );
    })
  );
  let totalFiles = [...table.querySelectorAll('div.entry:not(.hidden)')].length;
  let totalFileCount = html('span.total-file-count', `${totalFiles} file${totalFiles !== 1 ? 's' : ''}`);
  let selectedFileCount = html('span.selected-file-count.hidden');
  let downloadFiles = html('button.button.primary', 'Download');
  downloadFiles.disabled = true;
  table.bulkEvents = new BulkFilesOperations(table, entry.id, {
    totalFileCount,
    selectedFileCount,
    downloadFiles,
  });
  table.sorter = new TableSorter(table);
  let showHiddenFiles = html('button.button', 'Show Watched Episodes');
  showHiddenFiles.addEventListener('click', () => {
    let show = showHiddenFiles.textContent.startsWith('Show');
    table.querySelectorAll('.entry.filtered-episode').forEach(e => e.classList.toggle('hidden', !show));
    showHiddenFiles.textContent = show ? 'Hide Watched Episodes' : 'Show Watched Episodes';
    table.bulkEvents.updateFileCounts();
  });
  return html('details.anilist-entry',
    html('summary',
      html('a.cover', {href: `https://anilist.co/anime/${data.mediaId}/`},
        html('img', {loading: 'lazy', src: data.media.coverImage.medium, alt: `Cover image for ${entry.name}`})),
      entryLink(entry),
      html('span.progress', `${data.progress}/${data.media.episodes ?? '?'}`)),
    html('div.contents', table,
      html('div.commands',
        html('div.file-count', totalFileCount, selectedFileCount),
        html('div.command-buttons', isHiding ? showHiddenFiles : null, downloadFiles),
      )
    )
  );
}

async function getFullRelations(anilistIds) {
  let js = await callApi('/entry/relations/full', {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
    },
    body: JSON.stringify({anilist_ids: anilistIds})
  });
  if(js === null) {
    throw new Error('An error occurred, sorry.');
  }
  return js;
}

async function loadMorePlanning(planning) {
  loadMoreButton.disabled = true;
  let sublist = planning.slice(afterIndex, afterIndex + 250);
  let hasMore = sublist.length > 250;
  let js;
  try {
    js = await getFullRelations(sublist.map(e => e.mediaId));
  } catch(e) {
    loadMoreButton.textContent = `Error: ${e}. Try again in 3 seconds...`;
    await sleep(3000);
    loadMoreButton.disabled = false;
    return;
  }

  let lookup = Object.fromEntries(js.map(e => [e.entry.anilist_id, e]));
  for(const e of sublist) {
    if(lookup.hasOwnProperty(e.mediaId)) {
      let data = lookup[e.mediaId];
      entriesElement.appendChild(anilistEntryToElement(e, data.entry, data.files));
    }
  }

  if(hasMore) {
    afterIndex += 250;
  } else {
    loadMoreButton.classList.add('hidden');
  }
}

async function fillData(entries) {
  let anilistIds = entries.filter(d => d.status !== "PLANNING").map(d => d.mediaId);
  let js;
  try {
    js = await getFullRelations(anilistIds);
  } catch(e) {
    loadingElement.textContent = e.toString();
    return;
  }

  let lookup = Object.fromEntries(js.map(e => [e.entry.anilist_id, e]));
  let watching = entries.filter(d => d.status !== "PLANNING");
  entriesElement.appendChild(html('h2', 'Watching'));
  if(watching.length !== 0) {
    let children = [];
    for(const e of watching) {
      if(lookup.hasOwnProperty(e.mediaId)) {
        let data = lookup[e.mediaId];
        let el = anilistEntryToElement(e, data.entry, data.files);
        el.sortByValue = Date.parse(data.entry.last_modified);
        children.push(el);
      }
    }
    children.sort((a, b) => b.sortByValue - a.sortByValue);
    children.forEach(el => entriesElement.appendChild(el));
  } else {
    entriesElement.appendChild(html('p', 'Nothing found...'));
  }

  let planning = entries.filter(d => d.status === "PLANNING" && d.media.status !== "NOT_YET_RELEASED");
  entriesElement.appendChild(html('h2', 'Planning'));
  if(planning.length === 0) {
    entriesElement.appendChild(html('p', 'Nothing found...'));
  }
  else {
    let hasMore = planning.length > 250;
    let sublist = planning.slice(0, 250);
    try {
      js = await getFullRelations(sublist.map(d => d.mediaId));
    } catch(e) {
      entriesElement.appendChild(html('p', 'An error occurred fetching the information, sorry.'));
      return;
    }
    let lookup = Object.fromEntries(js.map(e => [e.entry.anilist_id, e]));
    for(const e of sublist) {
      if(lookup.hasOwnProperty(e.mediaId)) {
        let data = lookup[e.mediaId];
        entriesElement.appendChild(anilistEntryToElement(e, data.entry, data.files));
      }
    }
    if(hasMore) {
      afterIndex = 250;
      loadMoreButton.addEventListener('click', () => loadMorePlanning(planning));
      loadMoreButton.classList.remove('hidden');
    }
  }

  entriesElement.classList.remove('hidden');
  loadingElement.classList.add('hidden');
}

async function loadData() {
  await init();
  let entries;
  try {
    entries = await getAniListEntries(entriesElement.dataset.username);
  } catch(e) {
    return;
  }
  await fillData(entries);
}

document.addEventListener('DOMContentLoaded', loadData);

