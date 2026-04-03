// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'ffi.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$AgentEventDto {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is AgentEventDto);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'AgentEventDto()';
}


}

/// @nodoc
class $AgentEventDtoCopyWith<$Res>  {
$AgentEventDtoCopyWith(AgentEventDto _, $Res Function(AgentEventDto) __);
}


/// Adds pattern-matching-related methods to [AgentEventDto].
extension AgentEventDtoPatterns on AgentEventDto {
/// A variant of `map` that fallback to returning `orElse`.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case _:
///     return orElse();
/// }
/// ```

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( AgentEventDto_TextDelta value)?  textDelta,TResult Function( AgentEventDto_ToolCall value)?  toolCall,TResult Function( AgentEventDto_ToolResult value)?  toolResult,TResult Function( AgentEventDto_ContextStats value)?  contextStats,TResult Function( AgentEventDto_TurnSummary value)?  turnSummary,TResult Function( AgentEventDto_Done value)?  done,required TResult orElse(),}){
final _that = this;
switch (_that) {
case AgentEventDto_TextDelta() when textDelta != null:
return textDelta(_that);case AgentEventDto_ToolCall() when toolCall != null:
return toolCall(_that);case AgentEventDto_ToolResult() when toolResult != null:
return toolResult(_that);case AgentEventDto_ContextStats() when contextStats != null:
return contextStats(_that);case AgentEventDto_TurnSummary() when turnSummary != null:
return turnSummary(_that);case AgentEventDto_Done() when done != null:
return done(_that);case _:
  return orElse();

}
}
/// A `switch`-like method, using callbacks.
///
/// Callbacks receives the raw object, upcasted.
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case final Subclass2 value:
///     return ...;
/// }
/// ```

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( AgentEventDto_TextDelta value)  textDelta,required TResult Function( AgentEventDto_ToolCall value)  toolCall,required TResult Function( AgentEventDto_ToolResult value)  toolResult,required TResult Function( AgentEventDto_ContextStats value)  contextStats,required TResult Function( AgentEventDto_TurnSummary value)  turnSummary,required TResult Function( AgentEventDto_Done value)  done,}){
final _that = this;
switch (_that) {
case AgentEventDto_TextDelta():
return textDelta(_that);case AgentEventDto_ToolCall():
return toolCall(_that);case AgentEventDto_ToolResult():
return toolResult(_that);case AgentEventDto_ContextStats():
return contextStats(_that);case AgentEventDto_TurnSummary():
return turnSummary(_that);case AgentEventDto_Done():
return done(_that);}
}
/// A variant of `map` that fallback to returning `null`.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case _:
///     return null;
/// }
/// ```

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( AgentEventDto_TextDelta value)?  textDelta,TResult? Function( AgentEventDto_ToolCall value)?  toolCall,TResult? Function( AgentEventDto_ToolResult value)?  toolResult,TResult? Function( AgentEventDto_ContextStats value)?  contextStats,TResult? Function( AgentEventDto_TurnSummary value)?  turnSummary,TResult? Function( AgentEventDto_Done value)?  done,}){
final _that = this;
switch (_that) {
case AgentEventDto_TextDelta() when textDelta != null:
return textDelta(_that);case AgentEventDto_ToolCall() when toolCall != null:
return toolCall(_that);case AgentEventDto_ToolResult() when toolResult != null:
return toolResult(_that);case AgentEventDto_ContextStats() when contextStats != null:
return contextStats(_that);case AgentEventDto_TurnSummary() when turnSummary != null:
return turnSummary(_that);case AgentEventDto_Done() when done != null:
return done(_that);case _:
  return null;

}
}
/// A variant of `when` that fallback to an `orElse` callback.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case _:
///     return orElse();
/// }
/// ```

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String text)?  textDelta,TResult Function( String name)?  toolCall,TResult Function( String name,  bool success)?  toolResult,TResult Function( int tokensBeforeTurn,  int tokensAfterPrune,  int messagesPruned,  int historyLen,  int pruningThreshold)?  contextStats,TResult Function( String summary)?  turnSummary,TResult Function()?  done,required TResult orElse(),}) {final _that = this;
switch (_that) {
case AgentEventDto_TextDelta() when textDelta != null:
return textDelta(_that.text);case AgentEventDto_ToolCall() when toolCall != null:
return toolCall(_that.name);case AgentEventDto_ToolResult() when toolResult != null:
return toolResult(_that.name,_that.success);case AgentEventDto_ContextStats() when contextStats != null:
return contextStats(_that.tokensBeforeTurn,_that.tokensAfterPrune,_that.messagesPruned,_that.historyLen,_that.pruningThreshold);case AgentEventDto_TurnSummary() when turnSummary != null:
return turnSummary(_that.summary);case AgentEventDto_Done() when done != null:
return done();case _:
  return orElse();

}
}
/// A `switch`-like method, using callbacks.
///
/// As opposed to `map`, this offers destructuring.
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case Subclass2(:final field2):
///     return ...;
/// }
/// ```

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String text)  textDelta,required TResult Function( String name)  toolCall,required TResult Function( String name,  bool success)  toolResult,required TResult Function( int tokensBeforeTurn,  int tokensAfterPrune,  int messagesPruned,  int historyLen,  int pruningThreshold)  contextStats,required TResult Function( String summary)  turnSummary,required TResult Function()  done,}) {final _that = this;
switch (_that) {
case AgentEventDto_TextDelta():
return textDelta(_that.text);case AgentEventDto_ToolCall():
return toolCall(_that.name);case AgentEventDto_ToolResult():
return toolResult(_that.name,_that.success);case AgentEventDto_ContextStats():
return contextStats(_that.tokensBeforeTurn,_that.tokensAfterPrune,_that.messagesPruned,_that.historyLen,_that.pruningThreshold);case AgentEventDto_TurnSummary():
return turnSummary(_that.summary);case AgentEventDto_Done():
return done();}
}
/// A variant of `when` that fallback to returning `null`
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case _:
///     return null;
/// }
/// ```

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String text)?  textDelta,TResult? Function( String name)?  toolCall,TResult? Function( String name,  bool success)?  toolResult,TResult? Function( int tokensBeforeTurn,  int tokensAfterPrune,  int messagesPruned,  int historyLen,  int pruningThreshold)?  contextStats,TResult? Function( String summary)?  turnSummary,TResult? Function()?  done,}) {final _that = this;
switch (_that) {
case AgentEventDto_TextDelta() when textDelta != null:
return textDelta(_that.text);case AgentEventDto_ToolCall() when toolCall != null:
return toolCall(_that.name);case AgentEventDto_ToolResult() when toolResult != null:
return toolResult(_that.name,_that.success);case AgentEventDto_ContextStats() when contextStats != null:
return contextStats(_that.tokensBeforeTurn,_that.tokensAfterPrune,_that.messagesPruned,_that.historyLen,_that.pruningThreshold);case AgentEventDto_TurnSummary() when turnSummary != null:
return turnSummary(_that.summary);case AgentEventDto_Done() when done != null:
return done();case _:
  return null;

}
}

}

/// @nodoc


class AgentEventDto_TextDelta extends AgentEventDto {
  const AgentEventDto_TextDelta({required this.text}): super._();
  

 final  String text;

/// Create a copy of AgentEventDto
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$AgentEventDto_TextDeltaCopyWith<AgentEventDto_TextDelta> get copyWith => _$AgentEventDto_TextDeltaCopyWithImpl<AgentEventDto_TextDelta>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is AgentEventDto_TextDelta&&(identical(other.text, text) || other.text == text));
}


@override
int get hashCode => Object.hash(runtimeType,text);

@override
String toString() {
  return 'AgentEventDto.textDelta(text: $text)';
}


}

/// @nodoc
abstract mixin class $AgentEventDto_TextDeltaCopyWith<$Res> implements $AgentEventDtoCopyWith<$Res> {
  factory $AgentEventDto_TextDeltaCopyWith(AgentEventDto_TextDelta value, $Res Function(AgentEventDto_TextDelta) _then) = _$AgentEventDto_TextDeltaCopyWithImpl;
@useResult
$Res call({
 String text
});




}
/// @nodoc
class _$AgentEventDto_TextDeltaCopyWithImpl<$Res>
    implements $AgentEventDto_TextDeltaCopyWith<$Res> {
  _$AgentEventDto_TextDeltaCopyWithImpl(this._self, this._then);

  final AgentEventDto_TextDelta _self;
  final $Res Function(AgentEventDto_TextDelta) _then;

/// Create a copy of AgentEventDto
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? text = null,}) {
  return _then(AgentEventDto_TextDelta(
text: null == text ? _self.text : text // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class AgentEventDto_ToolCall extends AgentEventDto {
  const AgentEventDto_ToolCall({required this.name}): super._();
  

 final  String name;

/// Create a copy of AgentEventDto
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$AgentEventDto_ToolCallCopyWith<AgentEventDto_ToolCall> get copyWith => _$AgentEventDto_ToolCallCopyWithImpl<AgentEventDto_ToolCall>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is AgentEventDto_ToolCall&&(identical(other.name, name) || other.name == name));
}


@override
int get hashCode => Object.hash(runtimeType,name);

@override
String toString() {
  return 'AgentEventDto.toolCall(name: $name)';
}


}

/// @nodoc
abstract mixin class $AgentEventDto_ToolCallCopyWith<$Res> implements $AgentEventDtoCopyWith<$Res> {
  factory $AgentEventDto_ToolCallCopyWith(AgentEventDto_ToolCall value, $Res Function(AgentEventDto_ToolCall) _then) = _$AgentEventDto_ToolCallCopyWithImpl;
@useResult
$Res call({
 String name
});




}
/// @nodoc
class _$AgentEventDto_ToolCallCopyWithImpl<$Res>
    implements $AgentEventDto_ToolCallCopyWith<$Res> {
  _$AgentEventDto_ToolCallCopyWithImpl(this._self, this._then);

  final AgentEventDto_ToolCall _self;
  final $Res Function(AgentEventDto_ToolCall) _then;

/// Create a copy of AgentEventDto
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? name = null,}) {
  return _then(AgentEventDto_ToolCall(
name: null == name ? _self.name : name // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class AgentEventDto_ToolResult extends AgentEventDto {
  const AgentEventDto_ToolResult({required this.name, required this.success}): super._();
  

 final  String name;
 final  bool success;

/// Create a copy of AgentEventDto
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$AgentEventDto_ToolResultCopyWith<AgentEventDto_ToolResult> get copyWith => _$AgentEventDto_ToolResultCopyWithImpl<AgentEventDto_ToolResult>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is AgentEventDto_ToolResult&&(identical(other.name, name) || other.name == name)&&(identical(other.success, success) || other.success == success));
}


@override
int get hashCode => Object.hash(runtimeType,name,success);

@override
String toString() {
  return 'AgentEventDto.toolResult(name: $name, success: $success)';
}


}

/// @nodoc
abstract mixin class $AgentEventDto_ToolResultCopyWith<$Res> implements $AgentEventDtoCopyWith<$Res> {
  factory $AgentEventDto_ToolResultCopyWith(AgentEventDto_ToolResult value, $Res Function(AgentEventDto_ToolResult) _then) = _$AgentEventDto_ToolResultCopyWithImpl;
@useResult
$Res call({
 String name, bool success
});




}
/// @nodoc
class _$AgentEventDto_ToolResultCopyWithImpl<$Res>
    implements $AgentEventDto_ToolResultCopyWith<$Res> {
  _$AgentEventDto_ToolResultCopyWithImpl(this._self, this._then);

  final AgentEventDto_ToolResult _self;
  final $Res Function(AgentEventDto_ToolResult) _then;

/// Create a copy of AgentEventDto
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? name = null,Object? success = null,}) {
  return _then(AgentEventDto_ToolResult(
name: null == name ? _self.name : name // ignore: cast_nullable_to_non_nullable
as String,success: null == success ? _self.success : success // ignore: cast_nullable_to_non_nullable
as bool,
  ));
}


}

/// @nodoc


class AgentEventDto_ContextStats extends AgentEventDto {
  const AgentEventDto_ContextStats({required this.tokensBeforeTurn, required this.tokensAfterPrune, required this.messagesPruned, required this.historyLen, required this.pruningThreshold}): super._();
  

 final  int tokensBeforeTurn;
 final  int tokensAfterPrune;
 final  int messagesPruned;
 final  int historyLen;
 final  int pruningThreshold;

/// Create a copy of AgentEventDto
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$AgentEventDto_ContextStatsCopyWith<AgentEventDto_ContextStats> get copyWith => _$AgentEventDto_ContextStatsCopyWithImpl<AgentEventDto_ContextStats>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is AgentEventDto_ContextStats&&(identical(other.tokensBeforeTurn, tokensBeforeTurn) || other.tokensBeforeTurn == tokensBeforeTurn)&&(identical(other.tokensAfterPrune, tokensAfterPrune) || other.tokensAfterPrune == tokensAfterPrune)&&(identical(other.messagesPruned, messagesPruned) || other.messagesPruned == messagesPruned)&&(identical(other.historyLen, historyLen) || other.historyLen == historyLen)&&(identical(other.pruningThreshold, pruningThreshold) || other.pruningThreshold == pruningThreshold));
}


@override
int get hashCode => Object.hash(runtimeType,tokensBeforeTurn,tokensAfterPrune,messagesPruned,historyLen,pruningThreshold);

@override
String toString() {
  return 'AgentEventDto.contextStats(tokensBeforeTurn: $tokensBeforeTurn, tokensAfterPrune: $tokensAfterPrune, messagesPruned: $messagesPruned, historyLen: $historyLen, pruningThreshold: $pruningThreshold)';
}


}

/// @nodoc
abstract mixin class $AgentEventDto_ContextStatsCopyWith<$Res> implements $AgentEventDtoCopyWith<$Res> {
  factory $AgentEventDto_ContextStatsCopyWith(AgentEventDto_ContextStats value, $Res Function(AgentEventDto_ContextStats) _then) = _$AgentEventDto_ContextStatsCopyWithImpl;
@useResult
$Res call({
 int tokensBeforeTurn, int tokensAfterPrune, int messagesPruned, int historyLen, int pruningThreshold
});




}
/// @nodoc
class _$AgentEventDto_ContextStatsCopyWithImpl<$Res>
    implements $AgentEventDto_ContextStatsCopyWith<$Res> {
  _$AgentEventDto_ContextStatsCopyWithImpl(this._self, this._then);

  final AgentEventDto_ContextStats _self;
  final $Res Function(AgentEventDto_ContextStats) _then;

/// Create a copy of AgentEventDto
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? tokensBeforeTurn = null,Object? tokensAfterPrune = null,Object? messagesPruned = null,Object? historyLen = null,Object? pruningThreshold = null,}) {
  return _then(AgentEventDto_ContextStats(
tokensBeforeTurn: null == tokensBeforeTurn ? _self.tokensBeforeTurn : tokensBeforeTurn // ignore: cast_nullable_to_non_nullable
as int,tokensAfterPrune: null == tokensAfterPrune ? _self.tokensAfterPrune : tokensAfterPrune // ignore: cast_nullable_to_non_nullable
as int,messagesPruned: null == messagesPruned ? _self.messagesPruned : messagesPruned // ignore: cast_nullable_to_non_nullable
as int,historyLen: null == historyLen ? _self.historyLen : historyLen // ignore: cast_nullable_to_non_nullable
as int,pruningThreshold: null == pruningThreshold ? _self.pruningThreshold : pruningThreshold // ignore: cast_nullable_to_non_nullable
as int,
  ));
}


}

/// @nodoc


class AgentEventDto_TurnSummary extends AgentEventDto {
  const AgentEventDto_TurnSummary({required this.summary}): super._();
  

 final  String summary;

/// Create a copy of AgentEventDto
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$AgentEventDto_TurnSummaryCopyWith<AgentEventDto_TurnSummary> get copyWith => _$AgentEventDto_TurnSummaryCopyWithImpl<AgentEventDto_TurnSummary>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is AgentEventDto_TurnSummary&&(identical(other.summary, summary) || other.summary == summary));
}


@override
int get hashCode => Object.hash(runtimeType,summary);

@override
String toString() {
  return 'AgentEventDto.turnSummary(summary: $summary)';
}


}

/// @nodoc
abstract mixin class $AgentEventDto_TurnSummaryCopyWith<$Res> implements $AgentEventDtoCopyWith<$Res> {
  factory $AgentEventDto_TurnSummaryCopyWith(AgentEventDto_TurnSummary value, $Res Function(AgentEventDto_TurnSummary) _then) = _$AgentEventDto_TurnSummaryCopyWithImpl;
@useResult
$Res call({
 String summary
});




}
/// @nodoc
class _$AgentEventDto_TurnSummaryCopyWithImpl<$Res>
    implements $AgentEventDto_TurnSummaryCopyWith<$Res> {
  _$AgentEventDto_TurnSummaryCopyWithImpl(this._self, this._then);

  final AgentEventDto_TurnSummary _self;
  final $Res Function(AgentEventDto_TurnSummary) _then;

/// Create a copy of AgentEventDto
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? summary = null,}) {
  return _then(AgentEventDto_TurnSummary(
summary: null == summary ? _self.summary : summary // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class AgentEventDto_Done extends AgentEventDto {
  const AgentEventDto_Done(): super._();
  






@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is AgentEventDto_Done);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'AgentEventDto.done()';
}


}




// dart format on
