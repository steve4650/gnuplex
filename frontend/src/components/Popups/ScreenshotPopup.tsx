import { useEffect, useState } from "react";
import "./Popup.css";
import { API, type Screenshot } from "../../lib/API";

function ScreenshotPopup(props: { visible: boolean; closeHook: () => void }) {
  const [screenshots, setScreenshots] = useState<Screenshot[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  const refreshScreenshots = async () => {
    const res = await API.getRecentScreenshots(8);
    setScreenshots(res);
  };

  useEffect(() => {
    if (!props.visible) {
      return;
    }
    setError("");
    API.getRecentScreenshots(8)
      .then((res) => setScreenshots(res))
      .catch(() => {
        setError("Could not load recent screenshots.");
      });
  }, [props.visible]);

  if (!props.visible) {
    return null;
  }

  return (
    <div className="popup bg-white dark:bg-stone-800 m-5 min-w-80 max-w-3xl p-6">
      <div className="flex flex-col gap-4">
        <div>
          <div>
            <div className="header">Screenshots</div>
            <div className="text-sm text-black dark:text-white">
              Recent captures and the full screenshots directory.
            </div>
          </div>
        </div>
        <div className="flex flex-row flex-wrap gap-2">
          <a
            href="/screenshots/"
            target="_blank"
            rel="noreferrer"
            className="btn-standard px-3 py-2 text-center"
          >
            Open All Screenshots
          </a>
          <button
            type="button"
            className="btn-standard px-4 py-2"
            disabled={loading}
            onClick={async () => {
              setLoading(true);
              setError("");
              try {
                await API.takeScreenshot();
                await refreshScreenshots();
              } catch {
                setError("Could not take screenshot.");
              } finally {
                setLoading(false);
              }
            }}
          >
            {loading ? "Taking..." : "Take Screenshot"}
          </button>
          <button
            type="button"
            className="btn-standard px-4 py-2"
            onClick={() => props.closeHook()}
          >
            Close
          </button>
        </div>
        {error ? <div className="text-sm text-red-700 dark:text-red-300">{error}</div> : null}
        <div className="grid gap-3 max-h-96 overflow-y-auto">
          {screenshots.length === 0 ? (
            <div className="text-sm text-black dark:text-white">No screenshots yet.</div>
          ) : (
            screenshots.map((screenshot) => (
              <a
                key={screenshot.url}
                href={screenshot.url}
                target="_blank"
                rel="noreferrer"
                className="border border-stone-300 dark:border-stone-600 p-3 flex flex-row gap-3 items-center"
              >
                <img
                  src={screenshot.url}
                  alt={screenshot.name}
                  className="w-32 h-20 object-cover bg-stone-200 dark:bg-stone-700 shrink-0"
                />
                <div className="min-w-0">
                  <div className="font-mono text-xs break-all text-black dark:text-white">
                    {screenshot.name}
                  </div>
                  <div className="text-sm text-stone-600 dark:text-stone-300">
                    {new Date(screenshot.modified_at).toLocaleString()}
                  </div>
                </div>
              </a>
            ))
          )}
        </div>
      </div>
    </div>
  );
}

export { ScreenshotPopup };
