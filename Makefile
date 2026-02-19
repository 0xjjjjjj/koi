APP = target/Koi.app

build:
	cargo build --release

app: build
	rm -rf $(APP)
	mkdir -p $(APP)/Contents/MacOS
	mkdir -p $(APP)/Contents/Resources
	cp target/release/koi $(APP)/Contents/MacOS/koi
	cp bundle/Info.plist $(APP)/Contents/Info.plist
	cp bundle/koi.icns $(APP)/Contents/Resources/koi.icns
	@echo "Built $(APP)"

install: app
	rm -rf /Applications/Koi.app
	cp -r $(APP) /Applications/Koi.app
	@echo "Installed to /Applications/Koi.app"

clean:
	cargo clean

.PHONY: build app install clean
