const addStorageModal = document.getElementById('add-storage-modal');
const confirmAddButton = document.getElementById('confirm-add-button');

async function deleteSingleEntry(e) {
  e.preventDefault();
  const button = e.target;
  const tr = button.closest('tr');

  const key = button.dataset.key;
  const table = document.getElementById(key);
  if(table === null) {
    return;
  }

  let data = tableToJson(table);
  let deletedKey = tr.cells[0].textContent;
  delete data[deletedKey];

  let response = await callApi(`/admin/storage`, {
    method: 'POST',
    headers: {
      'content-type': 'application/json'
    },
    body: JSON.stringify({ key, data })
  });

  if(response === null) {
    return;
  }

  showAlert({level: 'success', content: `Successfully removed key ${deletedKey}.`});
  table.deleteRow(tr.rowIndex);
}

async function deleteAllWhitelist(e) {
  e.preventDefault();

  let response = await callApi(`/admin/storage`, {
    method: 'DELETE',
    headers: {
      'content-type': 'application/json'
    },
    body: JSON.stringify({ key: 'kitsunekko_whitelist' })
  });

  if(response === null) {
    return;
  }

  showAlert({level: 'success', content: `Successfully removed whitelisting.`});
  await sleep(1);
  window.location.reload();
}

function tableToJson(table) {
  let result = {};
  for(const row of table.rows) {
    let key = row.cells[0].textContent;
    let entryId = parseInt(row.cells[1].textContent, 10);
    if(isNaN(entryId)) {
      continue;
    }
    result[key] = entryId;
  }
  return result;
}

function addSingleEntry(e) {
  e.preventDefault();
  const button = e.target;
  const key = button.dataset.key;
  const label = button.dataset.label;

  addStorageModal.dataset.key = key;
  addStorageModal.querySelector('#storage-type-name').textContent = label;
  addStorageModal.showModal();
}

async function confirmAddEntry(e) {
  e.preventDefault();

  const table = document.getElementById(addStorageModal.dataset.key);
  let data = table === null ? {} : tableToJson(table);

  const key = document.getElementById('storage-key').value;
  const value = parseInt(document.getElementById('storage-entry-id').value, 10);
  data[key] = value;

  let response = await callApi(`/admin/storage`, {
    method: 'POST',
    headers: {
      'content-type': 'application/json'
    },
    body: JSON.stringify({
      key: addStorageModal.dataset.key,
      data,
    })
  });

  if(response === null) {
    return;
  }

  showAlert({level: 'success', content: 'Successfully added key, reloading in 1 second'});
  addStorageModal.close();

  await sleep(1000);
  window.location.reload();
}

document.querySelectorAll('.button.danger[data-key][data-value]').forEach(el => {
  el.addEventListener('click', deleteSingleEntry);
})

document.querySelectorAll('.button.primary[data-key][data-label]').forEach(el => {
  el.addEventListener('click', addSingleEntry);
})

addStorageModal.querySelector('button[formmethod=dialog]').addEventListener('click', e => {
  e.preventDefault();
  addStorageModal.close();
})

confirmAddButton.addEventListener('click', confirmAddEntry);

document.getElementById('clear-kitsunekko-whitelist').addEventListener('click', deleteAllWhitelist);
