import { useState } from "react";
import "./Popup.css";
import { API } from "../../lib/API";
import { useEscapeKey } from "../../lib/useEscapeKey";

function CastPopup(props: {
  visible: boolean;
  setCastPopup: React.Dispatch<React.SetStateAction<boolean>>;
  closeHook: () => void;
}) {
  const [url, setUrl] = useState("");
  const [addToLib, setAddToLib] = useState(false);

  useEscapeKey(() => {
    props.closeHook();
    props.setCastPopup(false);
  }, props.visible);
  if (props.visible) {
    return (
      <div className="popup bg-white dark:bg-stone-800 m-5">
        <div className="flex flex-row mb-2 items-center">
          <span className="header mr-1">URL</span>
          <input
            type="text"
            className="bg-cyan-800 text-slate-100 border border-black text-sm font-mono font-bold p-1"
            value={url}
            onChange={(e) => {
              API.cast(url, !addToLib);
              setUrl(e.target.value);
            }}
          />
        </div>
        <div className="flex flex-row items-center mb-2">
          <label>
            <span className="header mr-1">Add to Library</span>
            <input
              type="checkbox"
              className="ml-1"
              checked={addToLib}
              onChange={(e) => setAddToLib(e.target.checked)}
            />
          </label>
        </div>
        <div className="flex flex-row">
          <input
            type="button"
            value="OK"
            className="btn-standard mr-1"
            onClick={() => {
              API.cast(url, !addToLib);
              setUrl("");
              setAddToLib(false);
              props.setCastPopup(false);
            }}
          />
          <input
            type="button"
            value="Cancel"
            className="btn-standard"
            onClick={() => {
              props.closeHook();
              props.setCastPopup(false);
            }}
          />
        </div>
      </div>
    );
  }
  return null;
}

export { CastPopup };
