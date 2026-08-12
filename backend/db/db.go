package db

import (
	"database/sql"
	"gnuplex/models"
	"regexp"
	"strings"
	"time"

	"github.com/mattn/go-sqlite3"
	"gorm.io/driver/sqlite"
	"gorm.io/gorm"
	"gorm.io/gorm/clause"
	"gorm.io/gorm/logger"
)

func init() {
	sql.Register("sqlite3_extended", &sqlite3.SQLiteDriver{
		ConnectHook: func(conn *sqlite3.SQLiteConn) error {
			return conn.RegisterFunc("regexp", func(re, s string) bool {
				matched, err := regexp.MatchString(re, s)
				if err != nil {
					return false
				}
				return matched
			}, true)
		},
	})
}

type DB struct {
	ORM *gorm.DB
}

func Init(path string, verbose bool) (*DB, error) {
	logLevel := logger.Warn
	if verbose {
		logLevel = logger.Info
	}
	orm, err := gorm.Open(sqlite.New(sqlite.Config{
		DriverName: "sqlite3_extended",
		DSN:        path,
	}), &gorm.Config{
		Logger: logger.Default.LogMode(logLevel),
	})
	if err != nil {
		return nil, err
	}
	db := DB{ORM: orm}
	// migrations
	if err := db.ORM.AutoMigrate(&models.MediaItem{}); err != nil {
		return nil, err
	}
	if err := db.ORM.AutoMigrate(&models.MediaDir{}); err != nil {
		return nil, err
	}
	if err := db.ORM.AutoMigrate(&models.FileExtension{}); err != nil {
		return nil, err
	}
	return &db, nil
}

// Update a MediaItem's LastPlayed attribute to be the current time.
func (db *DB) UpdateLastPlayed(mediaItem *models.MediaItem) error {
	return db.ORM.
		Model(mediaItem).
		Update("LastPlayed", time.Now().UTC().Format(time.RFC3339)).Error
}

// Get the list of configured file extensions to exclude from media scans.
func (db *DB) GetFileExts() ([]models.FileExtension, error) {
	var exts []models.FileExtension
	err := db.ORM.Find(&exts).Error
	return exts, err
}

// Set the list of configured file extensions to exclude from media scans.
func (db *DB) SetFileExts(mediadirs []string) error {
	if err := db.ORM.Unscoped().Where("1 = 1").Delete(&models.FileExtension{}).Error; err != nil {
		return err
	}
	for _, dir := range mediadirs {
		if err := db.ORM.Clauses(clause.OnConflict{DoNothing: true}).
			Create(&models.FileExtension{Extension: strings.ToLower(dir)}).Error; err != nil {
			return err
		}
	}
	return nil
}

// Get the list of configured directories to scan for MediaItems.
func (db *DB) GetMediaDirs() ([]models.MediaDir, error) {
	var mediaDirs []models.MediaDir
	err := db.ORM.Order("path").Find(&mediaDirs).Error
	return mediaDirs, err
}

// Set the list of configured directories to scan for MediaItems.
func (db *DB) SetMediadirs(mediaDirs []string) error {
	mediaDirsDB, err := db.GetMediaDirs()
	if err != nil {
		return err
	}
	mediaDirsH := make(map[string]bool)
	for _, dir := range mediaDirs {
		mediaDirsH[dir] = true
	}
	for _, dir := range mediaDirs {
		err := db.ORM.Clauses(clause.OnConflict{DoNothing: true}).Create(&models.MediaDir{Path: dir}).Error
		if err != nil {
			return err
		}
	}
	for _, dir := range mediaDirsDB {
		if _, ok := mediaDirsH[dir.Path]; !ok {
			if err := db.ORM.Unscoped().Delete(&dir).Error; err != nil {
				return err
			}
		}
	}
	return err
}

// Get most recent 25 played MediaItems.
func (db *DB) GetLast25Played() ([]models.MediaItem, error) {
	var mediaItems []models.MediaItem
	err := db.ORM.
		Order("last_played desc").
		Limit(25).
		Where("last_played != ''").
		Find(&mediaItems).Error
	return mediaItems, err
}

// BuildSearchRegex converts a user search string into a safe, case-insensitive regex pattern.
// Successive spaces in search turn into .* in regex, and special regex characters are escaped.
func BuildSearchRegex(search string) string {
	fields := strings.Fields(search)
	if len(fields) == 0 {
		return ""
	}
	for i, f := range fields {
		fields[i] = regexp.QuoteMeta(f)
	}
	return "(?i)" + strings.Join(fields, ".*")
}

// Get all MediaItems which match the given search string.
func (db *DB) GetMediaItems(search string, offset int) ([]models.MediaItem, int64, error) {
	var mediaItems []models.MediaItem
	search = strings.TrimSpace(search)
	if search == "" {
		if err := db.ORM.
			Order("path").
			Limit(1000).
			Offset(offset).
			Find(&mediaItems).Error; err != nil {
			return nil, 0, err
		}
		var count int64
		if err := db.ORM.
			Model(models.MediaItem{}).
			Count(&count).Error; err != nil {
			return nil, 0, err
		}
		return mediaItems, count, nil
	}

	pattern := BuildSearchRegex(search)

	if err := db.ORM.
		Where("path REGEXP ?", pattern).
		Order("path").
		Limit(1000).
		Offset(offset).
		Find(&mediaItems).Error; err != nil {
		return nil, 0, err
	}
	var count int64
	if err := db.ORM.
		Model(models.MediaItem{}).
		Where("path REGEXP ?", pattern).
		Count(&count).Error; err != nil {
		return nil, 0, err
	}
	return mediaItems, count, nil
}

func (db *DB) GetMediaItemsByPaths(paths []string) ([]models.MediaItem, error) {
	var mediaItems []models.MediaItem
	if err := db.ORM.
		Where("path in ?", paths).
		Find(&mediaItems).Error; err != nil {
		return nil, err
	}
	return mediaItems, nil
}

func (db *DB) DeleteMediaItem(mediaItem *models.MediaItem) error {
	return db.ORM.Unscoped().Delete(mediaItem).Error
}

func (db *DB) DeleteMediaItemByPath(path string) error {
	return db.ORM.
		Unscoped().
		Where("path = ?", path).
		Delete(&models.MediaItem{}).Error
}

func (db *DB) DeleteMediaItemFilesNotMatchingUUID(uuid string) error {
	return db.ORM.
		Unscoped().
		Where("last_scan_uuid != ?", uuid).
		Where("type = ?", models.File).
		Delete(&models.MediaItem{}).Error
}

func (db *DB) AddMediaItemFile(path, lastScanUUID string) error {
	return db.ORM.
		Clauses(clause.OnConflict{
			Columns:   []clause.Column{{Name: "path"}},
			UpdateAll: true}).
		Create(&models.MediaItem{Path: path, LastScanUUID: lastScanUUID, Type: models.File}).Error
}

func (db *DB) AddMediaItemURL(url string) error {
	return db.ORM.
		Clauses(clause.OnConflict{
			Columns:   []clause.Column{{Name: "path"}},
			DoNothing: true}).
		Create(&models.MediaItem{Path: url, Type: models.URL}).Error
}
