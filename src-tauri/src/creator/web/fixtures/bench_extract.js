() => {
  // What a stealth-bench suite reports, read from the DOM (isolated world).
  const rb = document.querySelector("#detections-json");
  const out = document.querySelector("#out");
  return {
    url: location.href,
    title: document.title,
    text: (document.body ? document.body.innerText : "").slice(0, 30000),
    rebrowser: rb ? rb.value : null,
    interact: out ? out.textContent : null,
    sannysoftFailed: [...document.querySelectorAll("td.failed, td.warn")].map(
      (td) => td.className + ": " + (td.closest("tr") ? td.closest("tr").innerText : td.innerText)),
  };
}
