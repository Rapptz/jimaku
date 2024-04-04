const filterElement = document.getElementById('search-files');
const escapedRegex = /[-\/\\^$*+?.()|[\]{}]/g;
const escapeRegex = (e) => e.replace(escapedRegex, '\\$&');

function __score(haystack, query) {
  let result = fuzzysort.single(query, haystack);
  return result?.score == null ? -1000 : result.score;
}

const changeModifiedToRelative = () => {
  document.querySelectorAll('.file-modified').forEach(node => {
    const seconds = parseInt(node.parentElement.getAttribute('data-last-modified'), 10);
    node.textContent = formatRelative(seconds);
  });
}

const changeDisplayNames = (value) => {
  let keys = {'romaji':'data-name', 'native':'data-japanese-name', 'english':'data-english-name'};
  let key = keys[value];
  document.querySelectorAll('.entry > .file-name').forEach(el => {
    let parent = el.parentElement;
    el.textContent = parent.getAttribute(key) ?? parent.getAttribute('data-name');
  });

  let el = document.querySelector('.entry-info > .title');
  if (el !== null) {
    // entryData should be defined here
    el.textContent = getPreferredNameForEntry(entryData);
  }

  document.querySelectorAll('.relation.file-name').forEach(el => {
    el.textContent = el.getAttribute(key) ?? el.getAttribute('data-name');
  });
}

const parseEntryObjects = () => {
  document.querySelectorAll('.entry[data-extra]').forEach(el => {
    const obj = JSON.parse(el.getAttribute('data-extra'));
    for (const attr in obj) {
      if (obj[attr] === null) {
        continue;
      }
      if (attr == 'tmdb_id') {
        let value = obj[attr];
        el.setAttribute('data-tmdb-id', `${value.type}:${value.id}`);
      } else {
        el.setAttribute(`data-${attr.replaceAll('_', '-')}`, obj[attr]);
      }
    }
    el.removeAttribute('data-extra');
  });
};

function innerSortBy(attribute, ascending) {
  let entries = [...document.querySelectorAll('.entry')];
  if (entries.length === 0) {
    return;
  }
  let parent = entries[0].parentElement;
  entries.sort((a, b) => {
    if (attribute === 'data-name') {
      let firstName = a.textContent;
      let secondName = b.textContent;
      return ascending ? firstName.localeCompare(secondName) : secondName.localeCompare(firstName);
    } else {
      // The last two remaining sort options are either e.g. file.size or entry.last_modified
      // Both of these are numbers so they're simple to compare
      let first = parseInt(a.getAttribute(attribute), 10);
      let second = parseInt(b.getAttribute(attribute), 10);
      return ascending ? first - second : second - first;
    }
  });

  entries.forEach(obj => parent.appendChild(obj));
}

function sortBy(event, attribute) {
  // Check if the element has an ascending class tag
  // If it does, then when we're clicking on it we actually want to sort descending
  let ascending = !event.target.classList.contains('sorting-ascending');

  // Make sure to toggle everything else off...
  document.querySelectorAll('.table-headers > .table-header').forEach(node => node.classList.remove('sorting-ascending', 'sorting-descending'));

  // Sort the elements by what we requested
  innerSortBy(`data-${attribute}`, ascending);

  // Add the element class list depending on the operation we did
  let className = ascending ? 'sorting-ascending' : 'sorting-descending';
  event.target.classList.add(className);
}

let previousEntryOrder = null;

function resetSearchFilter() {
  if (filterElement.value.length === 0) {
    filterElement.focus();
  }

  let entries = [...document.querySelectorAll('.entry')];
  if (entries.length !== 0) {
    let parentNode = entries[0].parentNode;
    entries.forEach(e => e.classList.remove('hidden'));
    if (previousEntryOrder !== null) {
      previousEntryOrder.forEach(e => parentNode.appendChild(e));
      previousEntryOrder = null;
    }
  }

  filterElement.value = "";
}

const __scoreByName = (el, query) => {
  let total = __score(el.getAttribute('data-name'), query);
  let native = el.getAttribute('data-japanese-name');
  if (native !== null) {
    total = Math.max(total, __score(native, query));
  }
  let english = el.getAttribute('data-english-name');
  if (english !== null) {
    total = Math.max(total, __score(english, query));
  }
  return total;
}

function filterEntries(query) {
  if (!query) {
    resetSearchFilter();
    return;
  }

  let entries = [...document.querySelectorAll('.entry')];
  // Save the previous file order so it can be reset when we're done filtering
  if (previousEntryOrder === null) {
    previousEntryOrder = entries;
  }

  if (entries.length === 0) {
    return;
  }

  let parentNode = entries[0].parentNode;
  let anilistId = getAnilistId(query);
  let tmdb = getTmdbId(query);
  let mapped = [];
  if (anilistId !== null) {
    mapped = entries.map(e => {
      let id = e.getAttribute('data-anilist-id');
      return {
        entry: e,
        score: id !== null && parseInt(id, 10) === anilistId ? 0 : -1000,
      };
    });
  } else if (tmdb !== null) {
    let tmdbId = `${tmdb.type}:${tmdb.id}`;
    mapped = entries.map(e => {
      let id = e.getAttribute('data-tmdb-id');
      return {
        entry: e,
        score: id !== null && id == tmdbId ? 0 : -1000,
      };
    });
  } else {
    mapped = entries.map(e => {
      return {
        entry: e,
        score: __scoreByName(e, query),
      };
    })
  }

  mapped.sort((a, b) => b.score - a.score).forEach(el => {
    el.entry.classList.toggle('hidden', el.score <= -1000);
    parentNode.appendChild(el.entry);
  });
}

parseEntryObjects();
changeModifiedToRelative();
{
  let pref = localStorage.getItem('preferred_name') ?? 'romaji';
  if (pref != 'romaji') changeDisplayNames(pref);
}
innerSortBy('data-name', true);
settings.addEventListener('preferred-name', (e) => changeDisplayNames(e.detail));


document.getElementById('clear-search-filter')?.addEventListener('click', resetSearchFilter);
document.querySelectorAll('.table-header[data-sort-by]').forEach(el => {
  el.addEventListener('click', e => sortBy(e, el.getAttribute('data-sort-by')))
});
filterElement?.addEventListener('input', debounced(e => filterEntries(e.target.value)))
