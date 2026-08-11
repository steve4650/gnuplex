package db

import (
	"gnuplex/models"
	"path/filepath"
	"testing"
)

func TestBuildSearchRegex(t *testing.T) {
	tests := []struct {
		input    string
		expected string
	}{
		{"", ""},
		{"   ", ""},
		{"sopranos s02", "(?i)sopranos.*s02"},
		{"sopranos   s02", "(?i)sopranos.*s02"},
		{"  sopranos   s02  ", "(?i)sopranos.*s02"},
		{"sopranos (1999) s02", `(?i)sopranos.*\(1999\).*s02`},
		{"[1080p] movie", `(?i)\[1080p\].*movie`},
		{".* test +", `(?i)\.\*.*test.*\+`},
	}

	for _, tt := range tests {
		got := BuildSearchRegex(tt.input)
		if got != tt.expected {
			t.Errorf("BuildSearchRegex(%q) = %q; want %q", tt.input, got, tt.expected)
		}
	}
}

func TestGetMediaItems(t *testing.T) {
	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	database, err := Init(dbPath, false)
	if err != nil {
		t.Fatalf("Init failed: %v", err)
	}

	items := []models.MediaItem{
		{Path: "/media/Sopranos (1999) -    S02E01.mkv"},
		{Path: "/media/Sopranos S01E01.mkv"},
		{Path: "/media/The Wire S01E01.mkv"},
		{Path: "/media/Movie [1080p].mp4"},
		{Path: "/media/Show (2020) S01.mkv"},
	}
	for i := range items {
		if err := database.ORM.Create(&items[i]).Error; err != nil {
			t.Fatalf("Failed to create media item: %v", err)
		}
	}

	tests := []struct {
		search        string
		expectedCount int
		expectedFirst string
	}{
		{
			search:        "sopranos s02",
			expectedCount: 1,
			expectedFirst: "/media/Sopranos (1999) -    S02E01.mkv",
		},
		{
			search:        "sopranos    s02",
			expectedCount: 1,
			expectedFirst: "/media/Sopranos (1999) -    S02E01.mkv",
		},
		{
			search:        "SOPRANOS S02",
			expectedCount: 1,
			expectedFirst: "/media/Sopranos (1999) -    S02E01.mkv",
		},
		{
			search:        "sopranos",
			expectedCount: 2,
			expectedFirst: "/media/Sopranos (1999) -    S02E01.mkv",
		},
		{
			search:        "[1080p]",
			expectedCount: 1,
			expectedFirst: "/media/Movie [1080p].mp4",
		},
		{
			search:        "show (2020)",
			expectedCount: 1,
			expectedFirst: "/media/Show (2020) S01.mkv",
		},
		{
			search:        ".*",
			expectedCount: 0,
		},
		{
			search:        "",
			expectedCount: 5,
			expectedFirst: "/media/Movie [1080p].mp4",
		},
		{
			search:        "   ",
			expectedCount: 5,
			expectedFirst: "/media/Movie [1080p].mp4",
		},
	}

	for _, tt := range tests {
		res, count, err := database.GetMediaItems(tt.search, 0)
		if err != nil {
			t.Fatalf("GetMediaItems(%q) failed: %v", tt.search, err)
		}
		if int(count) != tt.expectedCount {
			t.Errorf("GetMediaItems(%q) count = %d; want %d", tt.search, count, tt.expectedCount)
		}
		if tt.expectedCount > 0 && (len(res) == 0 || res[0].Path != tt.expectedFirst) {
			actualFirst := ""
			if len(res) > 0 {
				actualFirst = res[0].Path
			}
			t.Errorf("GetMediaItems(%q) first item = %q; want %q", tt.search, actualFirst, tt.expectedFirst)
		}
	}
}
