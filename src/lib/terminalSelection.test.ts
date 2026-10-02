// @vitest-environment jsdom
import { expect, it } from "vitest";
import { tidySelectionHtml, trimSelectionLineEnds } from "./terminalSelection";

it("drops padding at line ends and keeps everything else", () => {
  expect(trimSelectionLineEnds("ls -la      \nREADME.md   \t\n")).toBe("ls -la\nREADME.md\n");
  expect(trimSelectionLineEnds("a  b   \r\nc   ")).toBe("a  b\r\nc");
  expect(trimSelectionLineEnds("  indented\n\n    \nlast")).toBe("  indented\n\n\nlast");
  expect(trimSelectionLineEnds("no padding")).toBe("no padding");
});

it("keeps the selection HTML's style and drops the same line-end padding", () => {
  const font = "'JetBrains Mono', 'Ubuntu Mono', monospace";
  const html =
    "<html><body><!--StartFragment--><pre>" +
    `<div style='color: #c9d1d9; background-color: #1e2228; font-family: ${font}; font-size: 14px;'>` +
    "<div><span>ls </span><span style='color: #7ee787;'>-la</span><span>      </span></div>" +
    "<div><span>  indented   </span></div>" +
    "<div><span>    </span></div>" +
    "</div></pre><!--EndFragment--></body></html>";
  const tidied = tidySelectionHtml(html, font);
  const parsed = new DOMParser().parseFromString(tidied, "text/html");
  const block = parsed.querySelector("pre > div") as HTMLElement;
  expect(block.style.fontFamily).toContain("JetBrains Mono");
  expect(block.style.backgroundColor).not.toBe("");
  const rows = [...parsed.querySelectorAll("pre > div > div")].map((row) => row.textContent);
  expect(rows).toEqual(["ls -la", "  indented", ""]);
  expect(parsed.querySelector("span[style]")?.textContent).toBe("-la");
  expect(tidied).toContain('<meta charset="utf-8">');
  expect(tidied).toContain("<!--StartFragment-->");
});

it("leaves selected text alone, even when it spells out the font or markup", () => {
  const font = "'JetBrains Mono', monospace";
  const html =
    "<html><body><!--StartFragment--><pre>" +
    `<div style='color: #fff; background-color: #000; font-family: ${font}; font-size: 14px;'>` +
    `<div><span>font: ${font} &lt;b&gt;x&amp;y</span></div>` +
    "</div></pre><!--EndFragment--></body></html>";
  const parsed = new DOMParser().parseFromString(tidySelectionHtml(html, font), "text/html");
  expect(parsed.querySelector("pre > div > div")?.textContent).toBe(`font: ${font} <b>x&y`);
  expect(parsed.querySelector("b")).toBeNull();
  expect((parsed.querySelector("pre > div") as HTMLElement).style.fontFamily).toContain("JetBrains Mono");
});
