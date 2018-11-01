#include "helpers.hpp"

extern "C" ASIOError get_sample_rate(double * rate){
	return ASIOGetSampleRate(reinterpret_cast<ASIOSampleRate *>(rate));
}

extern AsioDrivers* asioDrivers;
bool loadAsioDriver(char *name);

extern "C" bool load_asio_driver(char * name){
	return loadAsioDriver(name);
}

extern "C" void remove_current_driver() {
	asioDrivers->removeCurrentDriver();
}
extern "C" long get_driver_names(char **names, long maxDrivers) {
	AsioDrivers ad;
	return ad.getDriverNames(names, maxDrivers);
}