## 0.1.2
* bumped dirs to 3.0
* only allow 'c' and 'r' bindings to do things if app is not paused and is currently on a task
* hanging back on rodio 0.11 until [this](https://github.com/RustAudio/rodio/issues/290) is resolved
* add 'h' to toggle a help menu which describes keypresses
* ROUNDED CORNERS!!!

## 1.0.0
* increased the space for help page description
* bumped tui-rs to v0.10 + made TagCtr use ITALICS
* added space between 'DO THIS SHIT' and curr. task
* made the tag counter *italics*
* added 'match' logic for **c r i s p** screen switching
* added 'match' logic for **c r i s p** tag weight decision 
* fixed bug in recomputing of tag weights (#19)
* moved update_tag_weights to assignment_utils
* added 's' to toggle a stats menu which describes calculated probability of a given task
* unified highlight symbols

## 1.1.0
* made all boxes have the same margin
* add default config/task files if they don't exist (thanks to @javabird25!)
* bump tui version to 0.11

## 1.1.1
* error in tinytemplate for default files. rolling back till I fix it

## 1.1.2
* bump tui-rs to v0.12
* bump rodio to v0.12 + move some functions around to get it working
* fix tinytemplate default file shenanigans
* removed italics within the tag counter

## 1.1.3
* changed tagweight table to use percentages instead of floats

## 1.1.4
* removed Cargo.lock from .gitignore
* added some comments to tagweighttable function
* bumped tui-rs to v0.13
* bumped rodio to v0.13

## 1.2.0
* bumped tui-rs to v0.14
* modified the task table to use a stateful table in the backend
* bumped serde/serde_derive to 1.0.118
* bumped rand to v0.8
* bumpted tinytemplate to v1.2
* added -c/--config option
* create default config directories if they don't exist & no -c option

## 1.2.1
* bumped tinytemplate to v1.2.1
* bumped rand to v0.8.3
* bumped termion to v1.5.6
* bumped serde to v1.0.123
* remove serde_derive as dependency 
* formatted code and cleaned up some clippy warnings
