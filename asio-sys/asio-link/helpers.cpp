#include "helpers.hpp"

extern "C" void destruct_AsioDrivers(AsioDrivers * a){
	a->~AsioDrivers();
}

extern "C" ASIOError get_sample_rate(double * rate){
	return ASIOGetSampleRate(reinterpret_cast<ASIOSampleRate *>(rate));
}
