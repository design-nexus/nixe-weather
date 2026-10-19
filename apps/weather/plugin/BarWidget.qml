import QtQuick
import qs.Ui

// Toggles the Weather app; right-click opens its Settings card.
BarWidget {
  id: root
  moduleName: "design-nexus.weather"

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  WidgetButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: "󰖐"
    horizontalMargin: 7.5
    onPressed: function(button) {
      if (!root.bar) return
      if (button === Qt.RightButton) root.bar.run("nixe-weather --section settings")
      else root.bar.run("nixe-weather --toggle")
    }
  }
}
