# Create a PST from EML files using Outlook COM automation
# Works with classic Outlook (COM-based). New Outlook (web-based) may not support COM.

$emlFolder = "C:\Users\RyanB\Desktop\thundertest\PROMOTIONS_20260516-0706\PROMOTIONS"
$pstPath = "C:\dev\dedupe\fixtures\promotions_spam.pst"

# Clean up any existing PST
if (Test-Path $pstPath) {
    Remove-Item $pstPath -Force
}

Write-Host "Creating PST from EML files..."
Write-Host "Source: $emlFolder"
Write-Host "Destination: $pstPath"

# Start Outlook
$outlook = $null
try {
    $outlook = New-Object -ComObject Outlook.Application
} catch {
    Write-Error "Failed to start Outlook COM. If you have 'New Outlook', COM automation is not supported."
    Write-Error "Alternative: Use Thunderbird ImportExportTools NG or Aspose.Email to create the PST."
    exit 1
}

$namespace = $outlook.GetNamespace("MAPI")

# Create a new Unicode PST store
$namespace.AddStoreEx($pstPath, 2)  # 2 = olStoreUnicode

# Find the newly created PST
$store = $null
foreach ($s in $namespace.Stores) {
    if ($s.FilePath -eq $pstPath) {
        $store = $s
        break
    }
}

if ($store -eq $null) {
    Write-Error "Failed to locate created PST store"
    exit 1
}

# Add a subfolder
$rootFolder = $store.GetRootFolder()
$targetFolder = $rootFolder.Folders.Add("PROMOTIONS")

# Import EML files
$emlFiles = Get-ChildItem -Path $emlFolder -Filter "*.eml" | Sort-Object Name
$count = 0
$failed = 0

foreach ($file in $emlFiles) {
    try {
        # OpenSharedItem can read .eml files
        $item = $namespace.OpenSharedItem($file.FullName)

        # Determine item type and copy accordingly
        if ($item.MessageClass -eq "IPM.Note") {
            $mail = $item.Move($targetFolder)
        } else {
            $item.Move($targetFolder) | Out-Null
        }

        $count++
        if ($count % 10 -eq 0) {
            Write-Host "  Imported $count/$($emlFiles.Count) emails..."
        }
    } catch {
        $failed++
        Write-Warning "Failed to import $($file.Name): $_"
    }
}

Write-Host ""
Write-Host "Done! Imported $count emails ($failed failed) into:"
Write-Host "  $pstPath"
Write-Host "PST size: $([math]::Round((Get-Item $pstPath).Length / 1MB, 2)) MB"

# Save and close
$outlook.Quit()
[System.Runtime.Interopservices.Marshal]::ReleaseComObject($outlook) | Out-Null
