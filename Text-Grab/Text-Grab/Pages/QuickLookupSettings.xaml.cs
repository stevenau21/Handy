using System;
using System.Globalization;
using System.Windows;
using System.Windows.Controls;
using System.IO;
using Microsoft.Win32;
using Text_Grab.Properties;
using Text_Grab.Utilities;

namespace Text_Grab.Pages;
/// <summary>
/// Interaction logic for QuickLookupSettings.xaml
/// </summary>
public partial class QuickLookupSettings : Page
{
    private readonly Settings DefaultSettings = AppUtilities.TextGrabSettings;
    private bool _loaded = false;

    public QuickLookupSettings()
    {
        InitializeComponent();
    }

    private void Page_Loaded(object sender, RoutedEventArgs e)
    {
        LookupFileLocationTextBox.Text = DefaultSettings.LookupFileLocation;
        LookupSearchHistoryCheckBox.IsChecked = DefaultSettings.LookupSearchHistory;

        TryInsertCheckBox.IsChecked = DefaultSettings.TryInsert;

        InsertDelaySlider.Value = Math.Clamp(DefaultSettings.InsertDelay, InsertDelaySlider.Minimum, InsertDelaySlider.Maximum);
        InsertDelayValueText.Text = DefaultSettings.InsertDelay.ToString("0.0", CultureInfo.InvariantCulture);
        InsertDelaySlider.IsEnabled = DefaultSettings.TryInsert;

        _loaded = true;
    }

    private void LookupFileLocationTextBox_LostFocus(object sender, RoutedEventArgs e)
    {
        if (!_loaded) return;
        DefaultSettings.LookupFileLocation = LookupFileLocationTextBox.Text;
        DefaultSettings.Save();
    }

    private void BrowseLookupFileButton_Click(object sender, RoutedEventArgs e)
    {
        SaveFileDialog dlg = new()
        {
            AddExtension = true,
            DefaultExt = ".csv",
            Filter = "CSV files (*.csv)|*.csv",
            InitialDirectory = Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments),
            FileName = "QuickSimpleLookupDataFile.csv",
            OverwritePrompt = false
        };

        if (!string.IsNullOrEmpty(DefaultSettings.LookupFileLocation))
        {
            dlg.InitialDirectory = Path.GetDirectoryName(DefaultSettings.LookupFileLocation);
            dlg.FileName = Path.GetFileName(DefaultSettings.LookupFileLocation);
        }

        if (dlg.ShowDialog() == true)
        {
            LookupFileLocationTextBox.Text = dlg.FileName;
            DefaultSettings.LookupFileLocation = dlg.FileName;
            DefaultSettings.Save();
        }
    }

    private void LookupSearchHistoryCheckBox_Click(object sender, RoutedEventArgs e)
    {
        if (!_loaded) return;
        DefaultSettings.LookupSearchHistory = LookupSearchHistoryCheckBox.IsChecked == true;
        DefaultSettings.Save();
    }

    private void TryInsertCheckBox_Click(object sender, RoutedEventArgs e)
    {
        if (!_loaded) return;
        bool enabled = TryInsertCheckBox.IsChecked == true;
        DefaultSettings.TryInsert = enabled;
        DefaultSettings.Save();
        InsertDelaySlider.IsEnabled = enabled;
    }

    private void InsertDelaySlider_ValueChanged(object sender, RoutedPropertyChangedEventArgs<double> e)
    {
        if (!_loaded) return;
        double newVal = Math.Round(InsertDelaySlider.Value, 1);
        DefaultSettings.InsertDelay = newVal;
        DefaultSettings.Save();
        InsertDelayValueText.Text = newVal.ToString("0.0", CultureInfo.InvariantCulture);
    }
}
