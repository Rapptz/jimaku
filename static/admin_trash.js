/* This file is licensed under AGPL-3.0 */

function innerSortBy(attribute, ascending) {
  let entries = [...document.querySelectorAll('.entry')];
  if (entries.length === 0) {
    return;
  }
  let parent = entries[0].parentElement;
  entries.sort((a, b) => {
    if (attribute === 'data-name' || attribute === 'data-reason') {
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

function __scoreByName(el, query) {
  let total = Math.max(__score(el.dataset.name, query), __score(el.dataset.reason, query));
  let native = el.dataset.japaneseName;
  if (native !== null) {
    total = Math.max(total, __score(native, query));
  }
  let english = el.dataset.englishName;
  if (english !== null) {
    total = Math.max(total, __score(english, query));
  }
  return total;
}

async function processTrashRequest(action) {
  let files = getSelectedFiles().map(e => e.textContent);
  if (files.length === 0) {
    return;
  }

  let payload = {files, action};

  let js = await callApi(`/admin/trash`, {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
    },
    body: JSON.stringify(payload)
  });

  if(js === null) {
    return;
  }

  let total = js.success + js.failed;
  if (js.success != 0) {
    showAlert({level: 'success', content: `Successfully ${action}d ${js.success}/${total} files, refreshing in 3 seconds`});
    await sleep(3000);
    window.location.reload();
  } else {
    showAlert({level: 'error', content: `Failed to ${action} ${total} files.`});
  }
}

document.getElementById('trash-files').addEventListener('click', () => processTrashRequest('delete'));
document.getElementById('restore-files').addEventListener('click', () => processTrashRequest('restore'));
