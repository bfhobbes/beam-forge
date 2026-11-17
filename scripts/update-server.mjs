import { createServer } from "node:http";
import { createReadStream } from "node:fs";
import { readFile, stat } from "node:fs/promises";
import { resolve, basename } from "node:path";
import { pathToFileURL } from "node:url";
import semver from "semver";

// Local development only. Publishing is a filesystem operation, not a public API.
export function createUpdateServer(directory, port = 17845) {
  return createServer(async (request, response) => {
    response.setHeader("Cache-Control", "no-store");
    if (request.method !== "GET") {
      response.writeHead(405).end();
      return;
    }
    try {
      const url = new URL(request.url, `http://127.0.0.1:${port}`);
      const parts = url.pathname.split("/").filter(Boolean);
      if (parts[0] === "updates" && parts.length === 4) {
        const [, target, arch, current] = parts;
        if (!semver.valid(current)) {
          response.writeHead(400).end();
          return;
        }
        let releases;
        try {
          releases = JSON.parse(await readFile(resolve(directory, "releases.json"), "utf8"));
        } catch (error) {
          if (error.code !== "ENOENT") throw error;
          releases = {};
        }
        const release = releases[`${target}-${arch}`];
        if (!release || !semver.gt(release.version, current)) {
          response.writeHead(204).end();
          return;
        }
        response.writeHead(200, { "Content-Type": "application/json" });
        response.end(
          JSON.stringify({
            version: release.version,
            notes: release.notes,
            pub_date: release.pub_date,
            signature: release.signature,
            url: `http://127.0.0.1:${port}/artifacts/${encodeURIComponent(release.file)}`,
          }),
        );
        return;
      }
      if (parts[0] === "artifacts" && parts.length === 2) {
        const file = decodeURIComponent(parts[1]);
        if (file !== basename(file) || /[\\/:]/.test(file) || file.startsWith(".")) {
          response.writeHead(400).end();
          return;
        }
        const location = resolve(directory, "artifacts", file);
        const info = await stat(location);
        response.writeHead(200, {
          "Content-Type": "application/octet-stream",
          "Content-Length": info.size,
        });
        const stream = createReadStream(location);
        stream.on("error", () => response.destroy());
        response.on("close", () => stream.destroy());
        stream.pipe(response);
        return;
      }
      response.writeHead(404).end();
    } catch (error) {
      response.writeHead(error.code === "ENOENT" ? 404 : 500).end();
    }
  });
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  createUpdateServer(resolve(".local/update-server")).listen(17845, "127.0.0.1", () => {
    console.log("Demo update service: http://127.0.0.1:17845 (Ctrl+C to stop)");
  });
}
