# Whishlist

## core
* [x] Sync key to add potential new files from the source directory
* [x] Sync presets collections, everything
* [x] remove caching functionality and caching fields of clips
* [x] add audio plugin pipeline
* [ ] create trim tool, that trims on a specific threshold, similar to the analysis tool in the audio plugins
* [ ] create workflow to export slices, add bool to slice tool, to export all slices
* [ ] add zero crossing cuts to edits
* [ ] load plugins and structural edits dynamically not at compile time
* [ ] integrate vst plugins, and maybe replace own plugin system

## cli
* [x] add collection feature
* [x] display folder name and tags in list
* [ ] export/import function for collections and presets
* [x] export audio files in a certain format
* [x] now i want you to integrate the audio plugins in the cli client, they should be listed in the processors list. the list should also show the type, structural or audio-plugin. also i should be able to add them through the editor in presets and clips
* [x] remove plugin dependencies from the cli, should only be in the core library. there should be a registry that exposes the available plugins and processors and that lets you update edits and there should be a an engine function to update processors and plugins while its playing. i want to reuse this interface also with the tauri gui at a later point, so please design that interface to be reusable
* [ ] list available output devices and add option for player to play on a specific one
* [ ] add option to start player with a certain preset without writing it to the database
* [x] document code completion setup


## gui
* [ ] choose ui framework
* [ ] Filemanager like interface to manage source files, collections, clips and presets with a sidebar
* [ ] Display all items as rows or cards
* [ ] Allow selection of multiple files, collections and clips to do batch operations