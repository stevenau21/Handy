using System.Drawing;
using Text_Grab;
using Text_Grab.Models;
using Text_Grab.Utilities;

namespace Tests;

public class FilesIoTests
{
    private const string fontSamplePath = @"Images\font_sample.png";

    [WpfFact]
    public async Task CanSaveImagesWithHistory()
    {
        string path = FileUtilities.GetPathToLocalFile(fontSamplePath);
        Bitmap fontSampleBitmap = new(path);

        bool couldSave = await FileUtilities.SaveImageFile(fontSampleBitmap, "newTest.png", FileStorageKind.WithHistory);

        Assert.True(couldSave);
    }

    [WpfFact]
    public async Task SaveImageFile_SucceedsAfterClearTransientImage()
    {
        // Reproduces the race condition: SaveImageFile returns a Task that
        // may still be running when ClearTransientImage nulls the bitmap.
        // The save must complete successfully even when ClearTransientImage
        // is called immediately after the fire-and-forget pattern used by
        // HistoryService.SaveToHistory.
        string path = FileUtilities.GetPathToLocalFile(fontSamplePath);
        Bitmap bitmap = new(path);

        HistoryInfo historyInfo = new()
        {
            ID = "save-race-test",
            ImageContent = bitmap,
            ImagePath = $"race_test_{Guid.NewGuid()}.bmp",
        };

        Task<bool> saveTask = FileUtilities.SaveImageFile(
            historyInfo.ImageContent, historyInfo.ImagePath, FileStorageKind.WithHistory);

        // Mirrors what HistoryService.SaveToHistory does right after the
        // fire-and-forget call — must not cause saveTask to fail.
        historyInfo.ClearTransientImage();

        bool couldSave = await saveTask;
        Assert.True(couldSave);
    }

    [WpfFact]
    public async Task CanSaveTextFilesWithExe()
    {
        string textContent = "abcdef";
        string fileName = "testAbc.txt";

        bool couldSave = await FileUtilities.SaveTextFile(textContent, fileName, FileStorageKind.WithExe);
        Assert.True(couldSave);
    }

    [WpfTheory]
    [InlineData(FileStorageKind.WithExe)]
    [InlineData(FileStorageKind.WithHistory)]
    public async Task CanStoreThenReadTextFilesWithExe(FileStorageKind storageKind)
    {
        string textContent = $"Hello Hello this is a test of the system {DateTime.Now}";
        string fileName = "testAbc.txt";

        _ = await FileUtilities.SaveTextFile(textContent, fileName, storageKind);
        string readString = await FileUtilities.GetTextFileAsync(fileName, storageKind);

        Assert.Equal(textContent, readString);
    }

    [WpfTheory]
    [InlineData(FileStorageKind.WithExe)]
    [InlineData(FileStorageKind.WithHistory)]
    [InlineData(FileStorageKind.Absolute)]
    public async Task ReadNotExistingTextFileEmpty(FileStorageKind storageKind)
    {
        string fileName = "FileNotFound.json";
        string emptyReturn = await FileUtilities.GetTextFileAsync(fileName, storageKind);
        Assert.Empty(emptyReturn);
    }

    [WpfTheory]
    [InlineData(FileStorageKind.WithExe)]
    [InlineData(FileStorageKind.WithHistory)]
    [InlineData(FileStorageKind.Absolute)]
    public async Task ReadNotExistingImageFileEmpty(FileStorageKind storageKind)
    {
        string fileName = "FileNotFound.json";
        Bitmap? emptyReturn = await FileUtilities.GetImageFileAsync(fileName, storageKind);
        Assert.Null(emptyReturn);
    }
}
