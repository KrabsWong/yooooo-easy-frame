# Easy Frame

Easy Frame is a native macOS app for turning PNG sequences into animated APNG images.


| Light | Dark | 
|:-------|:------|
| <img width="300" alt="image" src="https://github.com/user-attachments/assets/672fb03b-b9c7-4070-afc4-df915e5f2aeb" /> | <img width="300" alt="image" src="https://github.com/user-attachments/assets/2d8dfc09-cabd-4b06-bd2e-cf9fbd0a9ae9" /> |
| <img width="300" alt="image" src="https://github.com/user-attachments/assets/dbea843a-01c4-4e70-8cb2-596e69c090c6" /> | <img width="300" alt="image" src="https://github.com/user-attachments/assets/6415a406-be13-4e44-9783-664ed84832f0" /> |
| <img width="300" alt="image" src="https://github.com/user-attachments/assets/8a641ef9-015f-4084-ad39-96ef75c427f3" /> | <img width="300" alt="image" src="https://github.com/user-attachments/assets/acdaa53f-e6ef-446f-97d9-e23877ae9f1d" /> |
| <img width="300" alt="image" src="https://github.com/user-attachments/assets/2158ed4c-0eb3-4c86-bdcc-5b6342a698e8" /> | <img width="300" alt="image" src="https://github.com/user-attachments/assets/7619600c-4c01-495c-961c-12f4b663a452" /> |


## Features

- Add PNG frames by dragging them into the window or using the file picker.
- Reorder frames by dragging them in the sequence list.
- Remove individual frames or clear the entire sequence with confirmation.
- Preview any frame in a detached side window with full PNG metadata.
- Navigate frames with the keyboard arrow keys.
- Adjust playback interval and output quality before export.
- Export ordered PNG sequences to APNG with a bundled LGPL FFmpeg build.
- Validate the bundled encoder at startup without requiring a system installation.
- Follow the system light and dark appearance automatically.
- Switch the interface between Chinese and English in Settings; Chinese is the default.
- Check for signed Sparkle updates and install them through macOS release assets.

## Installation

Download the macOS asset from the latest GitHub release, open it, and move `Easy Frame.app` to your `Applications` folder.

Release builds are signed ad hoc because the project does not have an Apple Developer ID and is not notarized. macOS may block the first launch with a warning that the developer cannot be verified.

To open the app for the first time:

1. Find `Easy Frame.app` in `Applications`, hold `Control` and click it, then choose **Open**.
2. Click **Open** again in the warning dialog.

If macOS still blocks the app, open **System Settings > Privacy & Security**, find the Easy Frame warning in the **Security** section, and click **Open Anyway** or **Allow Anyway**. Confirm with your password or Touch ID when prompted, then launch the app again.

After the first launch, the app opens normally.
