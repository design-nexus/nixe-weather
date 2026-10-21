import QtQuick
import Quickshell.Io
import qs.Ui

// The current conditions and temperature (from `nixe-weather --waybar`, which reads the
// forecast cache and refreshes it when stale); click toggles Weather, right-click opens
// its settings.
BarWidget {
  id: root
  moduleName: "design-nexus.weather"

  property string label: "󰖐"

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  WidgetButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: root.label
    horizontalMargin: 7.5
    onPressed: function(button) {
      if (!root.bar) return
      if (button === Qt.RightButton) root.bar.run("nixe-weather --section settings")
      else root.bar.run("nixe-weather --toggle")
    }
  }

  Process {
    id: poll
    command: ["nixe-weather", "--waybar"]
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        try {
          var out = JSON.parse(text)
          if (out.text) root.label = out.text
        } catch (e) {}
      }
    }
  }

  Timer {
    interval: 5 * 60 * 1000
    repeat: true
    running: true
    triggeredOnStart: true
    onTriggered: poll.running = true
  }
}
