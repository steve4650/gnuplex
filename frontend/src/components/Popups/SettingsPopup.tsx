import { useEffect, useState } from "react";
import "./Popup.css";
import { API } from "../../lib/API";
import { WorkingSpinnerTSX } from "../WorkingSpinner";

const filters = [
  { label: "Black and White", value: "bw" },
  { label: "Grainy", value: "grainy" },
  { label: "8-Bit", value: "8bit" },
  { label: "Mirror", value: "mirror" },
  { label: "Sepia", value: "sepia" },
  { label: "Psychedelic", value: "psychedelic" },
  { label: "Tron", value: "tron" },
  { label: "#NoFilter", value: "neutral" },
];

function SettingsPopup(props: {
  visible: boolean;
  closeHook: () => void;
  refreshMediaItems: () => void;
}) {
  const [_subDelay, setSubDelay] = useState(0);
  const [subDelayText, setSubDelayText] = useState("0");
  const [isExternalSub, setIsExternalSub] = useState(false);
  const [mediadirs, setMediadirs] = useState("");
  const [fileExts, setFileExts] = useState("");
  const [refreshLibraryWorking, setRefreshLibraryWorking] = useState(false);
  const [saveMediadirsWorking, setSaveMediadirsWorking] = useState(false);
  const [saveFileExtsWorking, setSaveFileExtsWorking] = useState(false);

  const refreshSubDelay = async () => {
    try {
      const delay = await API.getSubDelay();
      setSubDelay(delay);
      setSubDelayText(delay.toString());
    } catch {
      setSubDelay(0);
      setSubDelayText("0");
    }
  };

  useEffect(() => {
    if (!props.visible) {
      return;
    }

    refreshSubDelay();
    API.getSubTracks()
      .then((tracks) => {
        if (tracks) {
          const selected = tracks.find((t) => t.selected);
          setIsExternalSub(selected ? selected.external : false);
        } else {
          setIsExternalSub(false);
        }
      })
      .catch(() => setIsExternalSub(false));

    API.getMediadirs().then((res) => {
      setMediadirs(res.map((item) => item.Path).join("\n"));
    });
    API.getFileExts().then((res) => {
      setFileExts(res.map((item) => item.Extension).join("\n"));
    });
  }, [props.visible]);

  if (!props.visible) {
    return null;
  }

  const saveLibrarySettings = () => {
    setSaveMediadirsWorking(true);
    setSaveFileExtsWorking(true);
    const mediadirsValue = mediadirs
      .trim()
      .split("\n")
      .filter((line) => !/^\s*$/.test(line))
      .map((line) => line.trim());
    const fileExtsValue = fileExts
      .trim()
      .split("\n")
      .filter((line) => !/^\s*$/.test(line))
      .map((line) => line.trim());

    API.setMediadirs(mediadirsValue).then(() => {
      setSaveMediadirsWorking(false);
      props.refreshMediaItems();
    });
    API.setFileExts(fileExtsValue).then(() => {
      setSaveFileExtsWorking(false);
      props.refreshMediaItems();
    });
  };

  const refreshLibrary = () => {
    setRefreshLibraryWorking(true);
    API.scanLib().then(() => {
      setRefreshLibraryWorking(false);
      props.refreshMediaItems();
    });
  };

  return (
    <div className="popup bg-white dark:bg-stone-800 dark:text-white m-5 min-w-80 max-w-5xl w-[min(95vw,72rem)] p-8 max-sm:min-w-0 max-sm:h-screen max-sm:p-6">
      <div className="flex flex-col gap-8">
        <div className="flex flex-col gap-6">
          <h1 className="header">Settings</h1>
          <select
            className="btn-standard w-auto self-start"
            onChange={(e) => API.setFilter(e.target.value)}
            defaultValue="neutral"
          >
            <option value="" disabled>
              Select Filter
            </option>
            {filters.map((f) => (
              <option key={f.value} value={f.value}>
                {f.label}
              </option>
            ))}
          </select>
          <label className="flex items-center gap-2 text-black dark:text-white text-sm">
            Sub Delay (s):
            <input
              type="text"
              inputMode="numeric"
              pattern="-?[0-9]*\.?[0-9]*"
              value={subDelayText}
              onChange={(e) => {
                const val = e.target.value;
                if (/^-?[0-9]*\.?[0-9]*$/.test(val)) {
                  setSubDelayText(val);
                }
              }}
              onBlur={(e) => {
                const parsed = parseFloat(e.target.value);
                const value = Number.isNaN(parsed) ? 0 : parsed;
                setSubDelay(value);
                setSubDelayText(value.toString());
                API.setSubDelay(value);
              }}
              className="btn-standard w-24"
            />
          </label>
          {isExternalSub && (
            <div>
              <button
                type="button"
                onClick={async () => {
                  await API.saveSubDelay();
                  await refreshSubDelay();
                }}
                className="btn-standard px-4"
                title="Permanently save subtitle changes to file"
              >
                Save Subtitle Changes
              </button>
            </div>
          )}
        </div>

        <div className="flex flex-col gap-4">
          <h1 className="header">Library</h1>
          <label className="flex flex-col gap-2">
            <span>Media Directories</span>
            <textarea
              value={mediadirs}
              onChange={(e) => setMediadirs(e.target.value)}
              className="border border-solid border-black p-1 dark:bg-cyan-700 dark:text-white"
              rows={8}
              placeholder="/mnt/externalssd/tv/twilight_zone/eye_of_the_beholder.av1"
            />
          </label>
          <label className="flex flex-col gap-2">
            <span>Excluded File Extensions</span>
            <textarea
              value={fileExts}
              onChange={(e) => setFileExts(e.target.value)}
              className="border border-solid border-black p-1 dark:bg-cyan-700 dark:text-white"
              rows={8}
              placeholder=".pdf"
            />
          </label>
          <div className="flex flex-wrap gap-3 items-center">
            <button type="button" className="btn-standard px-4" onClick={saveLibrarySettings}>
              Save Library Settings
            </button>
            <WorkingSpinnerTSX visible={saveFileExtsWorking || saveMediadirsWorking} />
            <button type="button" className="btn-standard px-4" onClick={refreshLibrary}>
              Refresh Library
            </button>
            <WorkingSpinnerTSX visible={refreshLibraryWorking} />
          </div>
        </div>

        <div className="flex gap-2 justify-start">
          <button type="button" className="btn-standard px-4" onClick={props.closeHook}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}

export { SettingsPopup };
