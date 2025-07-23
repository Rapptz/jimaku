/* This file is licensed under AGPL-3.0 */
import { init, parse } from "./anitomy.js";

const entriesElement = document.getElementById('anilist-entries');
const loadingElement = document.getElementById('loading');
const loadMoreButton = document.getElementById('load-more');
const dtFormat = new Intl.DateTimeFormat(undefined, {
  dateStyle: 'full',
  timeStyle: 'medium',
});
let animeRelationData = null;
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
          nextAiringEpisode {
            episode
            timeUntilAiring
            airingAt
          }
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

function simpleRelativeFormat(seconds) {
  let hours = Math.floor(seconds / 3600);
  let rem = seconds % 3600;
  let minutes = Math.floor(rem / 60);
  seconds = rem % 60;
  let days = Math.floor(hours / 24);
  hours = hours % 24;

  let parts = [];
  if(days > 0) {
    parts.push(`${days}d`);
  }
  if(hours > 0) {
    parts.push(`${hours}h`);
  }
  if(minutes > 0) {
    parts.push(`${minutes}m`);
  }
  if(seconds > 0 && parts.length < 3) {
    parts.push(`${seconds}s`);
  }
  return parts.join(' ');
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

function findEquivalentEpisode(anilistId, episode) {
  if(animeRelationData == null || episode == null || anilistId == null) {
    return null;
  }

  let rules = animeRelationData[anilistId];
  if(!Array.isArray(rules)) {
    return null;
  }

  const getBegin = (r) => r.begin ?? r.value;
  const getEnd = (r) => r.type === 'from' ? Number.MAX_SAFE_INTEGER : (r.end ?? r.value);

  for(const rule of rules) {
    let distance = episode - getBegin(rule.source);
    if(getEnd(rule.source) - episode >= 0) {
      let found = getBegin(rule.destination);
      if(found.type !== 'number') {
        found += distance;
      }
      if(found >= 1 && found <= getEnd(rule.destination)) {
        return found;
      }
    }
  }
  return null;
}

const getEpisode = (ep) => {
  if(ep) {
    if(Array.isArray(ep)) {
      return [parseInt(ep[0]), parseInt(ep[1])]
    } else {
      return parseInt(ep);
    }
  }
  return null;
}

const getEquivalentEpisode = (anilistId, ep) => {
  if(Array.isArray(ep)) {
    let b = findEquivalentEpisode(anilistId, ep[0]);
    let e = findEquivalentEpisode(anilistId, ep[1]);
    if(b == null || e == null) return null;
    return [b, e];
  }
  return findEquivalentEpisode(anilistId, ep);
}

function isValidFile(range, progress) {
  if(range != null) {
    if(Array.isArray(range)) {
      let nextEpisode = progress + 1;
      return nextEpisode >= range[0] && nextEpisode <= range[1];
    }
    return range > progress;
  }
  return true;
}

function maxEpisodeFound(previous, range) {
  if(range != null) {
    if(Array.isArray(range)) {
      return Math.max(previous, Math.max(range[0], range[1]));
    }
    return Math.max(previous, range);
  }
  return previous;
}

function removeFoundEpisodes(episodes, range) {
  if(range != null) {
    if(Array.isArray(range)) {
      for(let start = range[0]; start <= range[1]; ++start) {
        episodes.delete(start);
      }
    }
    episodes.delete(range);
  }
}

function anilistEntryToElement(data, payload) {
  let isHiding = false;
  let lastEntryEpisode = 0;
  let episodesInEntry = new Set();
  const entry = payload.entry;
  const files = payload.files;
  if(data.media.status === 'FINISHED' && data.media.episodes != null) {
    for(let i = 1; i <= data.media.episodes; ++i) {
      episodesInEntry.add(i);
    }
  }
  let table = html('div.files', {dataset: {columns: '4'}},
    html('div.table-headers',
      html('span.table-header', html('input.bulk-check', {type: 'checkbox', autocomplete: 'off'})),
      html('span.table-header.sorting-ascending', {dataset: {sortBy: 'name'}}, 'Name'),
      html('span.table-header', {dataset: {sortBy: 'size'}}, 'Size'),
      html('span.table-header', {dataset: {sortBy: 'last-modified'}}, 'Date'),
    ),
    files.map(file => {
      let date = Date.parse(file.last_modified);
      let parsed = parse(file.name);
      let episodes = getEpisode(parsed.episode);
      let equivalent = getEquivalentEpisode(entry.anilist_id, episodes);
      let hidden = data.progress !== 0 ?
        !isValidFile(episodes, data.progress) || (equivalent !== null && !isValidFile(equivalent, data.progress))
        : false;
      lastEntryEpisode = maxEpisodeFound(lastEntryEpisode, episodes);
      removeFoundEpisodes(episodesInEntry, episodes);
      removeFoundEpisodes(episodesInEntry, equivalent);
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
  let bookmarkButton = html('button.button', payload.bookmarked ? 'Remove Bookmark' : 'Bookmark');
  bookmarkButton.addEventListener('click', async (e) => {
    const method = payload.bookmarked ? 'DELETE' : 'PUT';
    const response = await fetch(`/entry/${entry.id}/bookmark`, {method});
    if(response.ok) {
      payload.bookmarked = !payload.bookmarked;
      bookmarkButton.textContent = payload.bookmarked ? 'Remove Bookmark' : 'Bookmark';
    }
  })
  let nextAiringEpisode = data.media.nextAiringEpisode?.episode;
  let formattedNextEpisode = nextAiringEpisode != null && data.media.episodes == null ? ` (${nextAiringEpisode - 1})` : "";
  let isCaughtUp = nextAiringEpisode != null && data.progress === (nextAiringEpisode - 1);
  let isSiteBehind = data.media.status === 'RELEASING' && nextAiringEpisode != null && lastEntryEpisode < (nextAiringEpisode - 1);
  let missingEpisodes = data.media.episodes === 1 ? (files.length === 0 ? 1 : 0) : episodesInEntry.size;
  let isEntryIncomplete = data.media.status === 'FINISHED' && data.media.episodes != null && missingEpisodes !== 0;

  let airingAt = data.media.nextAiringEpisode?.airingAt;
  let timeUntilAiring = data.media.nextAiringEpisode?.timeUntilAiring;
  let nextAiringCountdownEl = null;
  if(nextAiringEpisode != null && airingAt != null && timeUntilAiring != null) {
    // Episode <N> airs in <time>
    nextAiringCountdownEl = html('span.countdown',
      `Episode ${nextAiringEpisode} airs in ${simpleRelativeFormat(timeUntilAiring)}`,
      { title: dtFormat.format(new Date(airingAt * 1000)) }
    )
  }

  return html('details.anilist-entry', isCaughtUp ? {class: 'caught-up'} : null,
    html('summary',
      html('a.cover', {href: `https://anilist.co/anime/${data.mediaId}/`},
        html('img', {loading: 'lazy', src: data.media.coverImage.medium, alt: `Cover image for ${entry.name}`})),
      html('.description', entryLink(entry), nextAiringCountdownEl),
      isSiteBehind ? html('span.behind', '🐢', {title: 'Currently missing subtitles for the latest episodes for this series.', dataset: {lastEpisode: lastEntryEpisode}}) : null,
      isEntryIncomplete ? html('span.missing', '⚠️', {title: `This entry might be missing ${missingEpisodes} episode${missingEpisodes == 1 ? '' : 's'}.`, dataset: {missing: missingEpisodes}}) : null,
      html('span.progress', `${data.progress}/${data.media.episodes ?? '?'}${formattedNextEpisode}`)),
    html('div.contents', table,
      html('div.commands',
        html('div.file-count', totalFileCount, selectedFileCount),
        html('div.command-buttons',
          payload.bookmarked !== null ? bookmarkButton : null,
          isHiding ? showHiddenFiles : null,
          downloadFiles
        ),
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
      entriesElement.appendChild(anilistEntryToElement(e, data));
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
        let el = anilistEntryToElement(e, data);
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
        entriesElement.appendChild(anilistEntryToElement(e, data));
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

async function loadAnimeRelations() {
  let previous = Date.parse(localStorage.getItem('anime_relations_last_modified') ?? '1970-01-01');
  if(previous !== 0) {
    let r = await fetch('/anime-relations/date');
    if(r.status === 200) {
      let dates = await r.json();
      let date = Date.parse(dates.last_modified);
      if(date === previous) return JSON.parse(localStorage.getItem('anime_relations'));
    }
  }

  let current = await fetch('/anime-relations');
  if(current.status === 200) {
    let js = await current.json();
    localStorage.setItem('anime_relations_last_modified', js.last_modified);
    localStorage.setItem('anime_relations', JSON.stringify(js.relations));
    return js.relations;
  }
  return null;
}

async function loadData() {
  await init();
  animeRelationData = await loadAnimeRelations();
  let entries;
  try {
    entries = await getAniListEntries(entriesElement.dataset.username);
  } catch(e) {
    return;
  }
  await fillData(entries);
}

document.addEventListener('DOMContentLoaded', loadData);

