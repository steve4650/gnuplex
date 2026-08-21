package gnuplex

import (
	"gnuplex/models"
	"io/fs"
	"log"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/asticode/go-astisub"
	"github.com/google/uuid"
	"gorm.io/gorm/clause"
)

// Updates GNUPlex's stored MediaItems, using its configured MediaDirs.
func (gnuplex *GNUPlex) ScanLib() error {
	// Grab MediaDirs, FileExts from the database
	mediaDirs, err := gnuplex.DB.GetMediaDirs()
	if err != nil {
		return err
	}
	fileExts, err := gnuplex.DB.GetFileExts()
	if err != nil {
		return err
	}
	fileExtH := make(map[string]bool)
	for _, fileExt := range fileExts {
		fileExtH[fileExt.Extension] = true
	}
	// Add new stuff
	lastScanUUID := uuid.New().String()
	var batch []models.MediaItem
	for i, mediaDir := range mediaDirs {
		if (i%100 == 0) && (i != 0) {
			if err = gnuplex.processScanLibBatch(batch, lastScanUUID); err != nil {
				return err
			}
		}
		dir, err := os.Stat(mediaDir.Path)
		if (err == nil) && dir.IsDir() {
			if err = filepath.WalkDir(mediaDir.Path, func(path string, entry fs.DirEntry, err error) error {
				if err != nil {
					log.Println("Error 62b2b0e2-25d7-49db-a31a-5785c145229f: file read issue for ", path, ": ", err)
					return nil
				} else if !entry.IsDir() {
					ext := strings.ToLower(path[strings.LastIndex(path, ".")+1:])
					if _, match := fileExtH[ext]; !match {
						batch = append(batch, models.MediaItem{Path: path, Type: models.File, LastScanUUID: lastScanUUID})
					}
					return nil
				} else {
					return nil
				}
			}); err != nil {
				return err
			}
		} else if err != nil {
			log.Println("skipping", dir, "- could not stat this directory")
		}
	}
	if len(batch) != 0 {
		if err = gnuplex.processScanLibBatch(batch, lastScanUUID); err != nil {
			return err
		}
	}
	return gnuplex.DB.DeleteMediaItemFilesNotMatchingUUID(lastScanUUID)
}

func (gnuplex *GNUPlex) processScanLibBatch(batch []models.MediaItem, lastScanUUID string) error {
	return gnuplex.DB.ORM.
		Clauses(clause.OnConflict{
			Columns:   []clause.Column{{Name: "path"}},
			DoUpdates: clause.Assignments(map[string]any{"last_scan_uuid": lastScanUUID}),
		}).
		CreateInBatches(batch, 100).Error
}

// Returns the currently playing MediaItem
func (gnuplex *GNUPlex) GetNowPlaying() ([]models.MediaItem, error) {
	playlist, err := gnuplex.MPV.GetNowPlaying()
	if err != nil && err.Error() == "property unavailable" {
		return nil, nil
	} else if err != nil {
		return nil, err
	}
	var paths []string
	for _, entry := range playlist {
		paths = append(paths, entry.Filename)
	}
	mediaItems, err := gnuplex.DB.GetMediaItemsByPaths(paths)
	if err != nil {
		return nil, err
	}
	// Create a map for quick lookup and preserve playlist order with QueueIds
	mediaMap := make(map[string]*models.MediaItem)
	for i := range mediaItems {
		mediaMap[mediaItems[i].Path] = &mediaItems[i]
	}
	// Rebuild the slice in the order from the playlist
	var orderedItems []models.MediaItem
	for _, entry := range playlist {
		if item, exists := mediaMap[entry.Filename]; exists {
			item.QueueId = entry.Id
			orderedItems = append(orderedItems, *item)
		}
	}
	return orderedItems, nil
}

// Play a MediaItem from the library (by ID).
func (gnuplex *GNUPlex) PlayById(id models.MediaItemId, playNext, playLast bool) error {
	var mediaItem *models.MediaItem
	if err := gnuplex.DB.ORM.First(&mediaItem, id).Error; err != nil {
		return err
	}
	if mediaItem != nil {
		gnuplex.PlayQueue = []*models.MediaItem{mediaItem}
	}
	if err := gnuplex.MPV.SetNowPlaying(mediaItem.Path, playNext, playLast); err != nil {
		return err
	}
	if err := gnuplex.DB.UpdateLastPlayed(mediaItem); err != nil {
		return err
	}
	return nil
}

// Play a MediaItem from the library (by path/URL).
func (gnuplex *GNUPlex) playByPath(path string, tempUrl, playNext, playLast bool) error {
	if tempUrl {
		return gnuplex.MPV.SetNowPlaying(path, true, false)
	} else {
		var mediaItem *models.MediaItem
		if err := gnuplex.DB.ORM.First(&mediaItem, "path = ?", path).Error; err != nil {
			return err
		}
		if mediaItem != nil {
			gnuplex.PlayQueue = []*models.MediaItem{mediaItem}
		}
		if err := gnuplex.MPV.SetNowPlaying(mediaItem.Path, playNext, playLast); err != nil {
			return err
		}
		if err := gnuplex.DB.UpdateLastPlayed(mediaItem); err != nil {
			return err
		}
	}
	return nil
}

// Cast a URL to the media player. `temp` determines whether or not it should be added to your library.
func (gnuplex *GNUPlex) Cast(url string, tempUrl, playNext, playLast bool) error {
	if tempUrl {
		return gnuplex.playByPath(url, true, playNext, playLast)
	} else if err := gnuplex.DB.AddMediaItemURL(url); err != nil {
		return err
	} else {
		return gnuplex.playByPath(url, tempUrl, playNext, playLast)
	}
}

// Cycle subtitle track.
func (gnuplex *GNUPlex) GetSubs() ([]models.Track, error) {
	tracks, err := gnuplex.MPV.GetTracks()
	if err != nil {
		return nil, err
	}
	var res []models.Track
	for _, track := range tracks {
		if track.Type == "sub" {
			res = append(res, track)
		}
	}
	return res, nil
}

// Set subtitle visibility.
func (gnuplex *GNUPlex) SetSubVisibility(visible bool) error {
	return gnuplex.MPV.SetSubVisibility(visible)
}

// Set subtitle track.
func (gnuplex *GNUPlex) SetSubTrack(trackID int64) error {
	return gnuplex.MPV.SetSubTrack(trackID)
}

// Save subtitle delay permanently to the subtitle file.
func (gnuplex *GNUPlex) SaveSubDelay() error {
	delay, err := gnuplex.MPV.GetSubDelay()
	if err != nil {
		return err
	}
	if delay == 0 {
		return nil
	}

	filename, err := gnuplex.MPV.GetCurrentSubFilename()
	if err != nil {
		return err
	}
	if filename == "" {
		return nil
	}

	subs, err := astisub.OpenFile(filename)
	if err != nil {
		return err
	}

	subs.Add(time.Duration(-delay * float64(time.Second)))

	if err := subs.Write(filename); err != nil {
		return err
	}

	if err := gnuplex.MPV.SubReload(); err != nil {
		return err
	}

	return gnuplex.MPV.SetSubDelay(0)
}
