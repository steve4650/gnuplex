package mpv

import (
	"bufio"
	"encoding/json"
	"errors"
	"gnuplex/models"
	"log"
	"strings"
)

/*
 * Types
 */
type MPVQuery struct {
	Command []any `json:"command"`
}

type MPVQueryString struct {
	Command []string `json:"command"`
}

type MPVResponseBool struct {
	Data bool `json:"data"`
}

type MPVResponseString struct {
	Data string `json:"data"`
}

type MPVResponseInt struct {
	Data int `json:"data"`
}

type PlaylistEntry struct {
	Filename string `json:"filename"`
	Id       int    `json:"id"`
	Current  bool   `json:"current"`
}

type MPVGetResult[T bool | string | int | float64 | []models.Track | []PlaylistEntry] struct {
	Data      T      `json:"data"`
	RequestId int    `json:"request_id"`
	Error     string `json:"error"`
}

type MPVSetResult struct {
	RequestId int    `json:"request_id"`
	Error     string `json:"error"`
}

/*
 * MPV command private methods
 */

func processMPVGetResult[T bool | string | int | float64 | []models.Track | []PlaylistEntry](resBytes []byte, err error) (T, error) {
	var defaultVal T
	if err != nil {
		return defaultVal, err
	}
	var res MPVGetResult[T]
	err = json.Unmarshal(resBytes, &res)
	if err != nil {
		log.Println("7b289386-c838-457d-8545-2f56bddb3746 MPV reported error in API call:", err)
		return defaultVal, err
	} else if res.Error != "success" {
		// Don't report this type of error- this error reporting should be handled at the application layer
		return defaultVal, errors.New(res.Error)
	}
	return res.Data, nil

}

func processMPVSetResult(resBytes []byte, err error) error {
	if err != nil {
		return err
	}
	var res MPVSetResult
	err = json.Unmarshal(resBytes, &res)
	if err != nil {
		log.Println("3989f92e-5230-4403-a60c-1ee8429c0088 MPV reported error in API call:", err)
		return err
	} else if res.Error != "success" {
		log.Println("d1a6f614-5eb6-4fa5-aedb-31b1940cb58e MPV reported error in API call:", err)
		return errors.New(res.Error)
	}
	return nil

}

func (mpv *MPV) unixMsg(msg []byte) ([]byte, error) {
	mpv.Mu.Lock()
	defer mpv.Mu.Unlock()
	_, err := mpv.Conn.Write(append(msg, '\n'))
	if err != nil {
		return nil, err
	}
	scanner := bufio.NewScanner(mpv.Conn)
	for scanner.Scan() {
		line := scanner.Text()
		if strings.Contains(line, "request_id") {
			return []byte(line), nil
		}
	}
	return []byte{}, nil
}

func (mpv *MPV) getCmd(cmd []string) ([]byte, error) {
	query := MPVQueryString{Command: cmd}
	jsonData, err := json.Marshal(query)
	if err != nil {
		return []byte{}, nil
	}
	return mpv.unixMsg(jsonData)
}

func (mpv *MPV) setCmd(cmd []any) ([]byte, error) {
	query := MPVQuery{Command: cmd}
	jsonData, err := json.Marshal(query)
	if err != nil {
		return []byte{}, nil
	}
	return mpv.unixMsg(jsonData)
}

func (mpv *MPV) DeleteQueueEntry(id int) error {
	if err := mpv.saveLastWatched(); err != nil {
		return err
	}
	return processMPVSetResult(mpv.setCmd([]any{"playlist-remove", id}))
}

func (mpv *MPV) saveLastWatched() error {
	return processMPVSetResult(mpv.setCmd([]any{"write-watch-later-config"}))
}

/*
 * MPV command public methods
 */
func (mpv *MPV) Play() error {
	return processMPVSetResult(
		mpv.setCmd([]any{"set_property", "pause", false}),
	)
}

func (mpv *MPV) Pause() error {
	return processMPVSetResult(
		mpv.setCmd([]any{"set_property", "pause", true}),
	)
}

func (mpv *MPV) PlayPause() error {
	return processMPVSetResult(
		mpv.setCmd([]any{"cycle", "pause"}),
	)
}

func (mpv *MPV) Skip() error {
	if err := mpv.saveLastWatched(); err != nil {
		return err
	}
	playlist, err := mpv.GetNowPlaying()
	if err != nil {
		return err
	}
	if len(playlist) > 1 {
		err = processMPVSetResult(mpv.setCmd([]any{"playlist-next"}))
		if err != nil {
			return err
		}
	}
	playlist, err = mpv.GetNowPlaying()
	if err != nil {
		return err
	}
	return mpv.DeleteQueueEntry(0)
}

func (mpv *MPV) GetPaused() (bool, error) {
	return processMPVGetResult[bool](mpv.getCmd([]string{"get_property", "pause"}))
}

func (mpv *MPV) GetNowPlaying() ([]PlaylistEntry, error) {
	return processMPVGetResult[[]PlaylistEntry](mpv.getCmd([]string{"get_property", "playlist"}))
}

func (mpv *MPV) GetCurrentFilename() (string, error) {
	playlist, err := mpv.GetNowPlaying()
	if err != nil {
		return "", err
	}
	for _, entry := range playlist {
		if entry.Current {
			return entry.Filename, nil
		}
	}
	if len(playlist) == 0 {
		return "", errors.New("property unavailable")
	}
	return playlist[0].Filename, nil
}

func (mpv *MPV) SetNowPlaying(filepath string, playNext, playLast bool) error {
	if playNext {
		return processMPVSetResult(
			mpv.setCmd([]any{"loadfile", filepath, "insert-next-play"}),
		)
	} else if playLast {
		return processMPVSetResult(
			mpv.setCmd([]any{"loadfile", filepath, "append-play"}),
		)
	}
	return errors.New("GetNowPlaying called incorrectly; need to specify a mode")
}

func (mpv *MPV) GetVol() (int, error) {
	n, err := processMPVGetResult[float64](mpv.getCmd([]string{"get_property", "volume"}))
	if err != nil {
		return 0, err
	}
	return int(n), err
}

func (mpv *MPV) SetVol(vol int) error {
	return processMPVSetResult(
		mpv.setCmd([]any{"set_property", "volume", vol}),
	)
}

func (mpv *MPV) GetPos() (int, error) {
	n, err := processMPVGetResult[float64](mpv.getCmd([]string{"get_property", "time-pos"}))
	if err != nil {
		return 0, err
	}
	return int(n), err
}

func (mpv *MPV) GetTimeRemaining() (int, error) {
	n, err := processMPVGetResult[float64](mpv.getCmd([]string{"get_property", "time-remaining"}))
	if err != nil {
		return 0, err
	}
	return int(n), err

}

func (mpv *MPV) SetPos(pos int) error {
	return processMPVSetResult(
		mpv.setCmd([]any{"set_property", "time-pos", pos}),
	)
}

func (mpv *MPV) ScreenshotToFile(filename string) error {
	return processMPVSetResult(
		mpv.setCmd([]any{"screenshot-to-file", filename, "subtitles"}),
	)
}

func (mpv *MPV) GetTracks() ([]models.Track, error) {
	return processMPVGetResult[[]models.Track](mpv.getCmd([]string{"get_property", "track-list"}))
}

func (mpv *MPV) SetSubVisibility(visible bool) error {
	return processMPVSetResult(
		mpv.setCmd([]any{"set_property", "sub-visibility", visible}),
	)
}

func (mpv *MPV) SetSubTrack(trackID int64) error {
	return processMPVSetResult(
		mpv.setCmd([]any{"set_property", "sid", trackID}),
	)
}

func (mpv *MPV) GetSubDelay() (float64, error) {
	return processMPVGetResult[float64](mpv.getCmd([]string{"get_property", "sub-delay"}))
}

func (mpv *MPV) SetSubDelay(delay float64) error {
	return processMPVSetResult(
		mpv.setCmd([]any{"set_property", "sub-delay", delay}),
	)
}

func (mpv *MPV) SubSeek(skip int) error {
	return processMPVSetResult(
		mpv.setCmd([]any{"sub-seek", skip}),
	)
}

func (mpv *MPV) GetCurrentSubFilename() (string, error) {
	tracks, err := mpv.GetTracks()
	if err != nil {
		return "", err
	}
	for _, track := range tracks {
		if track.Type == "sub" && track.Selected && track.External {
			return track.ExternalFilename, nil
		}
	}
	return "", nil
}

func (mpv *MPV) SubReload() error {
	return processMPVSetResult(
		mpv.setCmd([]any{"sub-reload"}),
	)
}

func (mpv *MPV) Ping() (int, error) {
	return processMPVGetResult[int](mpv.getCmd([]string{"get_property", "pid"}))
}

func (mpv *MPV) SetFilter(filter string) error {
	filterCmd := ""
	switch strings.ToLower(strings.TrimSpace(filter)) {
	case "bw":
		filterCmd = "lavfi=[format=gray]"
	case "grainy":
		filterCmd = "scale=480:trunc(ow/a/2)*2,setsar=1:1,eq=saturation=0.8,noise=alls=20:allf=t+u"
	case "mirror":
		filterCmd = "lavfi=[split[back][front];[back]hflip[back];[front][back]hstack]"
	case "8bit":
		filterCmd = "lavfi=[scale=iw/20:-1,scale=iw*20:-1:flags=neighbor]"
	case "sepia":
		filterCmd = "lavfi=[colorchannelmixer=.393:.769:.189:0:.349:.686:.168:0:.272:.534:.131,eq=contrast=1.1:brightness=-0.02]"
	case "psychedelic":
		filterCmd = "lavfi=[lagfun=decay=0.95]"
	case "tron":
		filterCmd = "lavfi=[edgedetect=low=0.1:high=0.4]"
	}
	return processMPVSetResult(
		mpv.setCmd([]any{"set_property", "vf", filterCmd}),
	)
}
