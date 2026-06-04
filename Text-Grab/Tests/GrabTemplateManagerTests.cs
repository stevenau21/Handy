using System.IO;
using System.Text.Json;
using Text_Grab.Models;
using Text_Grab.Properties;
using Text_Grab.Utilities;

namespace Tests;

[Collection("Settings isolation")]
public class GrabTemplateManagerTests : IDisposable
{
    private readonly string _tempFilePath;
    private readonly string _tempImagesFolder;
    private readonly string _originalGrabTemplatesJson;
    private readonly bool _originalEnableFileBackedManagedSettings;
    private readonly bool? _originalTestPreferFileBackedMode;

    public GrabTemplateManagerTests()
    {
        _tempFilePath = Path.Combine(Path.GetTempPath(), $"GrabTemplates_Test_{Guid.NewGuid()}.json");
        _tempImagesFolder = Path.Combine(Path.GetTempPath(), $"GrabTemplateImages_Test_{Guid.NewGuid()}");
        _originalGrabTemplatesJson = Settings.Default.GrabTemplatesJSON;
        _originalEnableFileBackedManagedSettings = Settings.Default.EnableFileBackedManagedSettings;
        _originalTestPreferFileBackedMode = GrabTemplateManager.TestPreferFileBackedMode;

        GrabTemplateManager.TestFilePath = _tempFilePath;
        GrabTemplateManager.TestImagesFolderPath = _tempImagesFolder;
        GrabTemplateManager.TestPreferFileBackedMode = false;

        Settings.Default.GrabTemplatesJSON = string.Empty;
        Settings.Default.EnableFileBackedManagedSettings = false;
        Settings.Default.Save();
    }

    public void Dispose()
    {
        GrabTemplateManager.TestFilePath = null;
        GrabTemplateManager.TestImagesFolderPath = null;
        GrabTemplateManager.TestPreferFileBackedMode = _originalTestPreferFileBackedMode;

        Settings.Default.GrabTemplatesJSON = _originalGrabTemplatesJson;
        Settings.Default.EnableFileBackedManagedSettings = _originalEnableFileBackedManagedSettings;
        Settings.Default.Save();

        if (File.Exists(_tempFilePath))
            File.Delete(_tempFilePath);

        if (Directory.Exists(_tempImagesFolder))
            Directory.Delete(_tempImagesFolder, true);
    }

    [Fact]
    public void GetAllTemplates_WhenEmpty_ReturnsEmptyList()
    {
        List<GrabTemplate> templates = GrabTemplateManager.GetAllTemplates();
        Assert.Empty(templates);
    }

    [Fact]
    public void GetAllTemplates_BackfillsLegacyFromSidecarWhenLegacyMissing()
    {
        GrabTemplate template = CreateSampleTemplate("Recovered");
        File.WriteAllText(_tempFilePath, JsonSerializer.Serialize(new[] { template }));

        List<GrabTemplate> templates = GrabTemplateManager.GetAllTemplates();

        GrabTemplate recoveredTemplate = Assert.Single(templates);
        Assert.Equal(template.Id, recoveredTemplate.Id);
        Assert.Contains(template.Id, Settings.Default.GrabTemplatesJSON);
    }

    [Fact]
    public void GetAllTemplates_FileBackedModePrefersFileAndBackfillsLegacy()
    {
        GrabTemplateManager.TestPreferFileBackedMode = true;
        GrabTemplate legacyTemplate = CreateSampleTemplate("Legacy");
        GrabTemplate sidecarTemplate = CreateSampleTemplate("Sidecar");

        Settings.Default.GrabTemplatesJSON = JsonSerializer.Serialize(new[] { legacyTemplate });
        Settings.Default.Save();
        File.WriteAllText(_tempFilePath, JsonSerializer.Serialize(new[] { sidecarTemplate }));

        List<GrabTemplate> templates = GrabTemplateManager.GetAllTemplates();

        GrabTemplate preferredTemplate = Assert.Single(templates);
        Assert.Equal(sidecarTemplate.Id, preferredTemplate.Id);
        Assert.Contains(sidecarTemplate.Id, Settings.Default.GrabTemplatesJSON);
    }

    [Fact]
    public void SaveTemplates_WritesBothFileAndLegacySetting()
    {
        GrabTemplate template = CreateSampleTemplate("Invoice");

        GrabTemplateManager.SaveTemplates([template]);

        Assert.True(File.Exists(_tempFilePath));
        Assert.Contains(template.Id, File.ReadAllText(_tempFilePath));
        Assert.Contains(template.Id, Settings.Default.GrabTemplatesJSON);
    }

    [Fact]
    public void GetAllTemplates_AfterAddingTemplate_ReturnsSavedTemplate()
    {
        GrabTemplate template = CreateSampleTemplate("Invoice");
        GrabTemplateManager.AddOrUpdateTemplate(template);

        List<GrabTemplate> templates = GrabTemplateManager.GetAllTemplates();
        Assert.Single(templates);
        Assert.Equal("Invoice", templates[0].Name);
    }

    [Fact]
    public void GetTemplateById_ExistingId_ReturnsTemplate()
    {
        GrabTemplate template = CreateSampleTemplate("Business Card");
        GrabTemplateManager.AddOrUpdateTemplate(template);

        GrabTemplate? found = GrabTemplateManager.GetTemplateById(template.Id);

        Assert.NotNull(found);
        Assert.Equal(template.Id, found.Id);
        Assert.Equal("Business Card", found.Name);
    }

    [Fact]
    public void GetTemplateById_NonExistentId_ReturnsNull()
    {
        GrabTemplate? found = GrabTemplateManager.GetTemplateById("non-existent-id");
        Assert.Null(found);
    }

    [Fact]
    public void AddOrUpdateTemplate_AddNew_IncrementsCount()
    {
        GrabTemplateManager.AddOrUpdateTemplate(CreateSampleTemplate("T1"));
        GrabTemplateManager.AddOrUpdateTemplate(CreateSampleTemplate("T2"));

        List<GrabTemplate> templates = GrabTemplateManager.GetAllTemplates();
        Assert.Equal(2, templates.Count);
    }

    [Fact]
    public void AddOrUpdateTemplate_UpdateExisting_ReplacesByIdNotDuplicate()
    {
        GrabTemplate original = CreateSampleTemplate("Original Name");
        GrabTemplateManager.AddOrUpdateTemplate(original);

        original.Name = "Updated Name";
        GrabTemplateManager.AddOrUpdateTemplate(original);

        List<GrabTemplate> templates = GrabTemplateManager.GetAllTemplates();
        Assert.Single(templates);
        Assert.Equal("Updated Name", templates[0].Name);
    }

    [Fact]
    public void DeleteTemplate_ExistingId_RemovesTemplate()
    {
        GrabTemplate template = CreateSampleTemplate("ToDelete");
        GrabTemplateManager.AddOrUpdateTemplate(template);

        GrabTemplateManager.DeleteTemplate(template.Id);

        List<GrabTemplate> templates = GrabTemplateManager.GetAllTemplates();
        Assert.Empty(templates);
    }

    [Fact]
    public void DeleteTemplate_NonExistentId_DoesNotThrow()
    {
        GrabTemplateManager.AddOrUpdateTemplate(CreateSampleTemplate("Keeper"));
        GrabTemplateManager.DeleteTemplate("does-not-exist");

        Assert.Single(GrabTemplateManager.GetAllTemplates());
    }

    [Fact]
    public void DuplicateTemplate_ValidId_CreatesNewTemplateWithCopyPrefix()
    {
        GrabTemplate original = CreateSampleTemplate("My Template");
        GrabTemplateManager.AddOrUpdateTemplate(original);

        GrabTemplate? copy = GrabTemplateManager.DuplicateTemplate(original.Id);

        Assert.NotNull(copy);
        Assert.NotEqual(original.Id, copy.Id);
        Assert.Contains("(copy)", copy.Name);
        Assert.Equal(2, GrabTemplateManager.GetAllTemplates().Count);
    }

    [Fact]
    public void DuplicateTemplate_NonExistentId_ReturnsNull()
    {
        GrabTemplate? copy = GrabTemplateManager.DuplicateTemplate("not-there");
        Assert.Null(copy);
    }

    [Fact]
    public void CreateButtonInfoForTemplate_SetsTemplateId()
    {
        GrabTemplate template = CreateSampleTemplate("Card");

        Text_Grab.Models.ButtonInfo button = GrabTemplateManager.CreateButtonInfoForTemplate(template);

        Assert.Equal(template.Id, button.TemplateId);
        Assert.Equal("ApplyTemplate_Click", button.ClickEvent);
        Assert.Equal(template.Name, button.ButtonText);
    }

    [Fact]
    public void GetAllTemplates_CorruptJson_ReturnsEmptyList()
    {
        File.WriteAllText(_tempFilePath, "{ this is not valid json }}}");

        List<GrabTemplate> templates = GrabTemplateManager.GetAllTemplates();
        Assert.Empty(templates);
    }

    [Fact]
    public void GrabTemplate_IsValid_TrueWhenNameAndOutputTemplateSet()
    {
        GrabTemplate template = CreateSampleTemplate("Valid");
        Assert.True(template.IsValid);
    }

    [Fact]
    public void GrabTemplate_IsValid_TrueWhenNoRegionsButHasNameAndOutputTemplate()
    {
        GrabTemplate template = CreateSampleTemplate("Text Only");
        template.Regions.Clear();
        Assert.True(template.IsValid);
    }

    [Fact]
    public void GrabTemplate_IsValid_FalseWhenNameEmpty()
    {
        GrabTemplate template = CreateSampleTemplate(string.Empty);
        Assert.False(template.IsValid);
    }

    [Fact]
    public void GrabTemplate_IsValid_FalseWhenOutputTemplateEmpty()
    {
        GrabTemplate template = CreateSampleTemplate("No Output");
        template.OutputTemplate = string.Empty;
        Assert.False(template.IsValid);
    }

    [Fact]
    public void GrabTemplate_GetReferencedRegionNumbers_ParsesPlaceholders()
    {
        GrabTemplate template = CreateSampleTemplate("Multi");
        template.OutputTemplate = "{1} {2} {1:upper}";

        HashSet<int> referenced = template.GetReferencedRegionNumbers().ToHashSet();

        Assert.Contains(1, referenced);
        Assert.Contains(2, referenced);
        Assert.Equal(2, referenced.Count);
    }

    private static GrabTemplate CreateSampleTemplate(string name)
    {
        return new GrabTemplate
        {
            Id = Guid.NewGuid().ToString(),
            Name = name,
            Description = "Test template",
            OutputTemplate = "{1}",
            ReferenceImageWidth = 800,
            ReferenceImageHeight = 600,
            Regions =
            [
                new Text_Grab.Models.TemplateRegion
                {
                    RegionNumber = 1,
                    Label = "Field 1",
                    RatioLeft = 0.1,
                    RatioTop = 0.1,
                    RatioWidth = 0.5,
                    RatioHeight = 0.1,
                }
            ]
        };
    }
}
